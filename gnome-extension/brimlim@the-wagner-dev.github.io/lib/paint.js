// Pure Cairo painting for one provider cell: the ring, and the percentage
// under it.
//
// Separated from the widget on purpose: it takes a context, a scale and a
// provider, touches no Shell API, and so can be rendered to a PNG in a test
// harness — and read side by side with the Rust transliteration that draws
// the same cell for the layer-shell frontend.

import Cairo from 'gi://cairo';
import Pango from 'gi://Pango';
import PangoCairo from 'gi://PangoCairo';

import {Colors, setColor, usageColor} from './palette.js';

const START_ANGLE = -Math.PI / 2;
const TAU = 2 * Math.PI;

/** Centre `text` on (cx, y), and hand back its height. */
export function centeredText(cr, text, cx, y, size, bold, color) {
    const layout = PangoCairo.create_layout(cr);
    const description = Pango.FontDescription.from_string('Sans');
    description.set_absolute_size(size * Pango.SCALE);
    if (bold)
        description.set_weight(Pango.Weight.BOLD);
    layout.set_font_description(description);
    layout.set_text(text, -1);

    const [width, height] = layout.get_pixel_size();
    setColor(cr, color);
    cr.moveTo(Math.round(cx - width / 2), Math.round(y));
    PangoCairo.show_layout(cr, layout);
    cr.newPath();
    return height;
}

/**
 * The provider's mark. Drawn rather than set as text: at 38 logical pixels a
 * font glyph is at the mercy of whatever the user has installed, and these
 * shapes are simple enough to own.
 *
 * Pass a `phase` in 0..1 and the mark comes alive: the burst's spokes take
 * turns reaching out, so a working agent is a shimmer travelling round the
 * mark rather than a separate spinner drawn beside it. That is one moving
 * thing per cell instead of two, and the thing that moves is the thing that
 * says which assistant is busy.
 */
export function drawMark(cr, id, cx, cy, radius, alpha = 1, phase = null) {
    cr.setLineCap(1 /* round */);

    if (id === 'codex') {
        // A filled shape cannot shimmer spoke by spoke, so it breathes.
        const breath = phase === null
            ? 1
            : 0.93 + 0.07 * (0.5 + 0.5 * Math.cos(TAU * phase));
        setColor(cr, [1, 1, 1, 1], alpha);
        drawRosette(cr, cx, cy, radius * breath);
        return;
    }

    // A burst of spokes, the Claude mark's silhouette. Eleven is what makes
    // it read as a burst rather than as an asterisk.
    const spokes = 11;
    cr.setLineWidth(Math.max(1, radius * 0.17));
    for (let i = 0; i < spokes; i += 1) {
        const angle = (i / spokes) * TAU - Math.PI / 2;
        const inner = radius * 0.16;

        // At rest the spokes alternate long and short. While the agent is
        // working that alternation becomes a wave running round the mark.
        let outer = radius * (i % 2 === 0 ? 1.0 : 0.82);
        let lit = 1;
        if (phase !== null) {
            const wave = 0.5 + 0.5 * Math.cos(TAU * (phase - i / spokes));
            outer = radius * (0.74 + 0.3 * wave);
            lit = 0.4 + 0.6 * wave;
        }

        setColor(cr, [1, 1, 1, 1], alpha * lit);
        cr.newPath();
        cr.moveTo(cx + Math.cos(angle) * inner, cy + Math.sin(angle) * inner);
        cr.lineTo(cx + Math.cos(angle) * outer, cy + Math.sin(angle) * outer);
        cr.stroke();
    }
}

/**
 * A six-lobed rosette with a hexagonal hole — the OpenAI mark's silhouette,
 * not its knot. Filled rather than stroked on purpose: at the nineteen pixels
 * this is actually drawn at, an outlined knot turns to mush, and a shape that
 * survives being small is worth more than one that is accurate when blown up.
 */
function drawRosette(cr, cx, cy, radius) {
    const HOLE = 0.46;
    const BULGE = 1.32;

    const ring = r => {
        for (let i = 0; i < 6; i += 1) {
            const angle = (i / 6) * TAU - Math.PI / 2;
            const next = ((i + 1) / 6) * TAU - Math.PI / 2;
            const mid = (angle + next) / 2;
            const control = [cx + Math.cos(mid) * r * BULGE, cy + Math.sin(mid) * r * BULGE];

            if (i === 0)
                cr.moveTo(cx + Math.cos(angle) * r, cy + Math.sin(angle) * r);
            cr.curveTo(...control, ...control,
                cx + Math.cos(next) * r, cy + Math.sin(next) * r);
        }
        cr.closePath();
    };

    cr.setFillRule(Cairo.FillRule.EVEN_ODD);
    cr.newPath();
    ring(radius);
    ring(radius * HOLE);
    cr.fill();
    cr.setFillRule(Cairo.FillRule.WINDING);
}

/**
 * @param {object} cr Cairo context
 * @param {object} spec
 * @param {number} spec.ring   ring diameter, physical px
 * @param {number} spec.inset  room around the ring for the waiting pulse
 * @param {number} spec.scale  stage scale factor
 * @param {object} spec.provider a provider from the daemon's state
 * @param {number} spec.phase  0..1, spinner rotation
 * @param {number} spec.pulse  0..1, waiting-pulse brightness
 */
export function paintCell(cr, {ring, inset = 0, scale, provider, phase = 0, pulse = 0}) {
    const stroke = Math.round(4 * scale);
    const center = inset + ring / 2;
    const radius = ring / 2 - stroke / 2;

    const percent = provider.headline_percent;
    const hasNumber = typeof percent === 'number' && Number.isFinite(percent);
    const dimmed = provider.status === 'stale' ? 0.55 : 1;
    const known = provider.status === 'ok' || provider.status === 'stale';

    // The disc the mark sits on, a shade above the pill so the ring reads as
    // a control rather than as a hole.
    setColor(cr, Colors.disc);
    cr.newPath();
    cr.arc(center, center, radius - stroke / 2 - Math.round(2 * scale), 0, TAU);
    cr.fill();

    cr.setLineCap(1 /* round */);
    cr.setLineWidth(stroke);

    setColor(cr, Colors.track);
    cr.newPath();
    cr.arc(center, center, radius, 0, TAU);
    cr.stroke();

    if (hasNumber) {
        if (provider.fidelity === 'derived')
            cr.setDash([Math.round(3 * scale), Math.round(3 * scale)], 0);
        else if (provider.fidelity === 'manual')
            cr.setDash([Math.round(1 * scale), Math.round(4 * scale)], 0);

        setColor(cr, usageColor(percent), dimmed);
        cr.newPath();
        cr.arc(center, center, radius, START_ANGLE, START_ANGLE + TAU * Math.min(percent, 1));
        cr.stroke();
        cr.setDash([], 0);
    } else {
        // No reading: one muted tick at twelve o'clock. An empty ring would
        // read as zero, which is a number nobody measured.
        setColor(cr, Colors.unknown);
        cr.setLineWidth(Math.round(2 * scale));
        cr.newPath();
        cr.arc(center, center, radius, START_ANGLE - 0.12, START_ANGLE + 0.12);
        cr.stroke();
        cr.setLineWidth(stroke);
    }

    drawMark(
        cr, provider.id, center, center, radius * 0.48,
        known ? 1 : 0.5,
        provider.activity === 'busy' ? phase : null);

    if (provider.activity === 'waiting') {
        setColor(cr, Colors.waiting, 0.15 + 0.5 * pulse);
        cr.setLineWidth(Math.round(2 * scale));
        cr.newPath();
        cr.arc(center, center, radius + Math.round(3 * scale), 0, TAU);
        cr.stroke();
    }

    // The percentage, or a dash where there is no number to show.
    centeredText(
        cr,
        hasNumber ? `${Math.round(percent * 100)}%` : '—',
        center,
        inset + ring + Math.round(3 * scale),
        13 * scale,
        false,
        hasNumber ? [1, 1, 1, dimmed] : Colors.unknown);
}
