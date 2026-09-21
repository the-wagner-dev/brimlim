#!/usr/bin/gjs -m
// The reveal as a filmstrip, and the mark's working animation under it.
//
// An animation is the one thing a still screenshot cannot review, and the
// one thing a unit test cannot either: `revealShape` can be checked for
// monotonicity and bounds, but not for whether it looks like a drop. So the
// frames are laid out side by side, at the sizes they are really drawn at.
//
//   gjs -m tools/render-reveal.js out.png [scale]

import Cairo from 'gi://cairo';

import {Edge} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/edges.js';
import {Metrics, pillSize, revealShape, ringOrigin, scaled, setScale}
    from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/geometry.js';
import {drawMark, paintCell} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/paint.js';
import {drawPill, drawTongue} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/pill.js';

const [outPath = 'reveal.png', scaleArg = '1'] = ARGV;
const scale = Number(scaleArg);
setScale(scale);

const STEPS = [0, 0.15, 0.3, 0.42, 0.55, 0.7, 0.85, 1];
const PROVIDERS = [
    {id: 'claude', label: 'Claude', headline_percent: 0.73, fidelity: 'official', status: 'ok', activity: 'busy'},
    {id: 'codex', label: 'Codex', headline_percent: 0.21, fidelity: 'official', status: 'ok', activity: 'idle'},
    {id: 'claude', label: 'Claude', headline_percent: 0.52, fidelity: 'official', status: 'ok', activity: 'waiting'},
];

const EDGE = Edge.RIGHT;
const size = pillSize(EDGE, PROVIDERS.length);
const gutter = scaled(30);
const frameW = size[0] + gutter;
const frameH = size[1] + gutter;
const markRow = scaled(110);

const width = frameW * STEPS.length;
const height = frameH + markRow;

const surface = new Cairo.ImageSurface(Cairo.Format.ARGB32, width, height);
const cr = new Cairo.Context(surface);
cr.setSourceRGB(0.83, 0.85, 0.88);
cr.paint();

STEPS.forEach((t, index) => {
    const shape = revealShape(EDGE, size, t);

    cr.save();
    cr.translate(index * frameW + gutter / 2, gutter / 2);

    // The box the notch is allowed to grow inside, so a frame that overflows
    // it is visible rather than merely wrong in a test.
    cr.setSourceRGBA(0, 0, 0, 0.12);
    cr.setLineWidth(1);
    cr.rectangle(0.5, 0.5, size[0] - 1, size[1] - 1);
    cr.stroke();

    if (shape.tongue > 0) {
        const thickness = scaled(Metrics.tongueThickness);
        const length = scaled(Metrics.tongueLength);
        cr.save();
        cr.translate(size[0] - thickness, Math.round((size[1] - length) / 2));
        drawTongue(cr, EDGE, thickness, length, shape.tongue);
        cr.restore();
    }

    if (shape.width > 0 && shape.height > 0) {
        cr.save();
        cr.translate(shape.x, shape.y);
        drawPill(cr, EDGE, shape.width, shape.height,
            {flare: shape.flare, radius: shape.radius});
        cr.restore();
    }

    if (shape.cells > 0) {
        cr.pushGroup();
        PROVIDERS.forEach((provider, cell) => {
            const [rx, ry] = ringOrigin(EDGE, cell);
            cr.save();
            cr.translate(rx, ry);
            paintCell(cr, {
                ring: scaled(Metrics.ringSize),
                inset: scaled(Metrics.pulseRoom),
                scale,
                provider,
                phase: 0.3,
                pulse: 0.8,
            });
            cr.restore();
        });
        cr.popGroupToSource();
        cr.paintWithAlpha(shape.cells);
    }

    cr.restore();
});

// The mark's own animation, blown up: at cell size the wave is a shimmer,
// and a shimmer is hard to review one frame at a time.
const radius = scaled(38);
STEPS.forEach((phase, index) => {
    cr.save();
    cr.translate(index * frameW + frameW / 2, frameH + markRow / 2);
    cr.setSourceRGB(0, 0, 0);
    cr.arc(0, 0, radius * 1.4, 0, 2 * Math.PI);
    cr.fill();
    drawMark(cr, 'claude', 0, 0, radius, 1, phase);
    cr.restore();
});

cr.$dispose();
surface.flush();
surface.writeToPNG(outPath);
print(`wrote ${outPath} (${width}x${height}, scale ${scale})`);
