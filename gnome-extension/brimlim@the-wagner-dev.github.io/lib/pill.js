// The pill background: a dark slab whose corners invert where it meets the
// screen edge, so it reads as growing out of the bezel rather than floating
// in front of it.
//
// The shape is authored once for the right edge and transformed into the
// other three. The transforms are rotations and one reflection, so the arc
// directions stay consistent and the path never needs a second spelling.

import {Edge} from './edges.js';

/**
 * Right-edge pill, flush against x = w.
 *
 * `flare` is the inverted corner radius at the edge; `radius` the ordinary
 * rounding on the three free sides. The body is inset by `flare` along the
 * edge so the inverted corners have somewhere to live.
 */
function rightEdgePath(cr, w, h, flare, radius) {
    const f = Math.min(flare, h / 2);
    const r = Math.min(radius, w, (h - 2 * f) / 2);

    cr.newPath();
    // Top inverted corner: leaves the edge and curves into the body.
    cr.moveTo(w, 0);
    cr.arc(w - f, 0, f, 0, Math.PI / 2);
    // Body top edge, then the ordinary rounded corner away from the edge.
    cr.lineTo(r, f);
    cr.arcNegative(r, f + r, r, -Math.PI / 2, Math.PI);
    cr.lineTo(0, h - f - r);
    cr.arcNegative(r, h - f - r, r, Math.PI, Math.PI / 2);
    // Bottom edge, then the second inverted corner back to the bezel.
    cr.lineTo(w - f, h - f);
    cr.arc(w - f, h, f, -Math.PI / 2, 0);
    cr.closePath();
}

/** Run `draw(w, h)` in a frame where the screen edge is always on the right. */
function withEdgeFrame(cr, edge, width, height, draw) {
    cr.save();
    switch (edge) {
    case Edge.RIGHT:
        draw(width, height);
        break;
    case Edge.LEFT:
        cr.translate(width, 0);
        cr.scale(-1, 1);
        draw(width, height);
        break;
    case Edge.TOP:
        cr.translate(0, height);
        cr.rotate(-Math.PI / 2);
        draw(height, width);
        break;
    case Edge.BOTTOM:
    default:
        cr.translate(width, 0);
        cr.rotate(Math.PI / 2);
        draw(height, width);
        break;
    }
    cr.restore();
}

export function drawPill(cr, edge, width, height, {flare, radius, alpha = 1}) {
    withEdgeFrame(cr, edge, width, height, (w, h) => {
        rightEdgePath(cr, w, h, flare, radius);
        // Black, and no outline: the pill is meant to read as a piece of the
        // bezel that has grown out over the screen, not as a panel sitting
        // on top of it.
        cr.setSourceRGBA(0, 0, 0, alpha);
        cr.fill();
    });
}

/** The resting tongue, drawn with the same inverted-corner vocabulary. */
export function drawTongue(cr, edge, width, height, alpha = 1) {
    const radius = Math.min(width, height) / 2;
    cr.newPath();
    if (width >= height) {
        cr.arc(radius, radius, radius, Math.PI / 2, -Math.PI / 2);
        cr.arc(width - radius, radius, radius, -Math.PI / 2, Math.PI / 2);
    } else {
        cr.arc(radius, radius, radius, Math.PI, 0);
        cr.arc(radius, height - radius, radius, 0, Math.PI);
    }
    cr.closePath();
    cr.setSourceRGBA(1, 1, 1, 0.38 * alpha);
    cr.fill();
}
