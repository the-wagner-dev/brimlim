// Every number here is in logical pixels. Nothing in this file may be used
// raw: multiply by the stage scale factor at the point of use, which is what
// keeps a HiDPI screen identical to a 1x screen in logical terms.

import {Edge, isVertical} from './edges.js';

export {Edge, isVertical};

export const Metrics = {
    ringSize: 38,
    ringStroke: 4,
    // Room around the ring for the waiting pulse, which is drawn outside it.
    // Without this the animation is clipped by its own cell.
    pulseRoom: 4,
    // The percentage sits under its ring, so a provider occupies a cell
    // rather than a circle.
    percentGap: 3,
    percentHeight: 15,
    cellGap: 34,
    pillPadding: 12,
    pillRadius: 24,
    // The inverted corner where the pill meets the bezel. It extends the
    // shape along the edge, so the body keeps its full length.
    flare: 20,
    tongueThickness: 4,
    tongueLength: 80,
    cardGap: 10,
    cardWidth: 280,
    cardTail: 9,
};

export const Timing = {
    revealMs: 420,
    collapseMs: 260,
    collapseDelayMs: 400,
    autoRevealMs: 5000,
    spinMs: 1400,
    pulseMs: 1100,
};

/** A cell is a ring with room for its pulse, and its percentage underneath. */
export function cellWidth() {
    return Metrics.ringSize + 2 * Metrics.pulseRoom;
}

export function cellSize() {
    return Metrics.pulseRoom + Metrics.ringSize + Metrics.percentGap + Metrics.percentHeight;
}

// The stage scale lives here rather than being read from St at every call
// site: one place learns it, everything else is a pure function of it, and
// the geometry can then be tested without a Shell.
let currentScale = 1;

/** Called by the extension when the stage scale factor changes. */
export function setScale(scale) {
    currentScale = scale > 0 ? scale : 1;
}

export function scaleFactor() {
    return currentScale;
}

export function scaled(value) {
    return Math.round(value * scaleFactor());
}

/**
 * Pill size for a given provider count, in physical pixels.
 * The pill grows along the edge and stays a fixed depth away from it.
 */
export function pillSize(edge, count) {
    const pad = scaled(Metrics.pillPadding);
    const flare = scaled(Metrics.flare);
    const gap = scaled(Metrics.cellGap);
    const wide = scaled(cellWidth());
    const tall = scaled(cellSize());

    const [step, across] = isVertical(edge) ? [tall, wide] : [wide, tall];
    const body = count > 0 ? count * step + (count - 1) * gap + 2 * pad : step + 2 * pad;

    const along = body + 2 * flare;
    const deep = across + 2 * pad;

    return isVertical(edge) ? [deep, along] : [along, deep];
}

/**
 * Where the container sits on its monitor. It is anchored so that a fully
 * revealed pill sits flush against the edge; everything the notch does while
 * it is less than fully revealed happens inside this box, which is why the
 * container never moves.
 */
export function placement(edge, monitor, [width, height]) {
    switch (edge) {
    case Edge.LEFT:
        return {
            x: monitor.x,
            y: monitor.y + Math.round((monitor.height - height) / 2),
        };
    case Edge.RIGHT:
        return {
            x: monitor.x + monitor.width - width,
            y: monitor.y + Math.round((monitor.height - height) / 2),
        };
    case Edge.TOP:
        return {
            x: monitor.x + Math.round((monitor.width - width) / 2),
            y: monitor.y,
        };
    case Edge.BOTTOM:
    default:
        return {
            x: monitor.x + Math.round((monitor.width - width) / 2),
            y: monitor.y + monitor.height - height,
        };
    }
}

/**
 * Top-left of the cell at `index`, inside the pill. Cells are laid out by
 * coordinate now that each carries a label under its ring, and the GTK
 * frontend hit-tests against these numbers, so both ports must agree.
 */
export function ringOrigin(edge, index) {
    const pad = scaled(Metrics.pillPadding);
    const flare = scaled(Metrics.flare);
    const gap = scaled(Metrics.cellGap);
    const step = (isVertical(edge) ? scaled(cellSize()) : scaled(cellWidth())) + gap;
    const offset = flare + pad + index * step;

    return isVertical(edge) ? [pad, offset] : [offset, pad];
}

/** The always-present tongue, positioned inside the container. */
export function tongueBox(edge, [width, height]) {
    const thickness = scaled(Metrics.tongueThickness);
    const length = Math.min(scaled(Metrics.tongueLength), isVertical(edge) ? height : width);

    switch (edge) {
    case Edge.LEFT:
        return [0, Math.round((height - length) / 2), thickness, length];
    case Edge.RIGHT:
        return [width - thickness, Math.round((height - length) / 2), thickness, length];
    case Edge.TOP:
        return [Math.round((width - length) / 2), 0, length, thickness];
    case Edge.BOTTOM:
    default:
        return [Math.round((width - length) / 2), height - thickness, length, thickness];
    }
}

// -- the reveal -----------------------------------------------------------
//
// The notch does not slide out; it grows out. The shape is a pure function
// of one 0..1 progress value, which is what lets both frontends animate
// identically without sharing a line of drawing code.

export const Reveal = {
    // Phase boundaries along that single timeline. They overlap on purpose:
    // the drop is still swelling when it starts to spread, which is what
    // stops the two halves reading as two separate animations.
    dropEnd: 0.40,
    stretchFrom: 0.30,
    stretchTo: 0.88,
    cellsFrom: 0.72,
    // How far the drop flattens while it spreads. Surface tension, not a
    // bounce: the shape never grows past the pill's own box, because that
    // box is the whole of the room the container has.
    flatten: 0.07,
};

export function clamp01(value) {
    if (!Number.isFinite(value))
        return 0;
    return Math.min(1, Math.max(0, value));
}

export function easeOutCubic(t) {
    return 1 - Math.pow(1 - clamp01(t), 3);
}

/**
 * Slow at both ends. The stretch uses this rather than an ease-out so that
 * the drop is still a drop when it stops swelling: an ease-out is already a
 * fifth of the way along the edge by then, and the two beats blur into one
 * diagonal smear.
 */
export function easeInOutCubic(t) {
    const p = clamp01(t);
    return p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2;
}

/**
 * The pill's drawn — and owned — box at reveal progress `t`, inside the
 * container's own `[width, height]`.
 *
 * At `t = 0` it is nothing. It leaves the edge as a disc as deep as the pill
 * will be, then stretches along the edge into the full shape, and only then
 * do the cells fade in. `radius` and `flare` are handed back with it because
 * the rounding is part of the morph: a drop is round, a pill is not.
 */
export function revealShape(edge, [width, height], t) {
    const progress = clamp01(t);
    const vertical = isVertical(edge);
    const full = vertical ? width : height;   // depth, away from the edge
    const span = vertical ? height : width;   // length, along the edge

    const swell = easeOutCubic(progress / Reveal.dropEnd);
    const stretch = easeInOutCubic(
        (progress - Reveal.stretchFrom) / (Reveal.stretchTo - Reveal.stretchFrom));

    // The drop grows out of the tongue's own footprint, and is about as long
    // as it is deep, so it leaves the edge round. Seeding it with the longer
    // of the two is not a detail: a drop shorter than the tongue would leave
    // the pointer that summoned it outside the shape, and the notch would
    // collapse under the very pointer holding it open.
    const seed = Math.max(full, scaled(Metrics.tongueLength));
    const along = Math.min(span, seed + (span - seed) * stretch);
    const depth = full * swell * (1 - Reveal.flatten * Math.sin(Math.PI * stretch));

    const w = Math.round(vertical ? depth : along);
    const h = Math.round(vertical ? along : depth);

    // Half the short side is a disc; the pill's own radius is the end state.
    // The min covers both directions, since which one is larger depends on
    // how deep this edge's pill is.
    const half = Math.min(w, h) / 2;
    const radius = Math.min(half, half + (scaled(Metrics.pillRadius) - half) * stretch);
    const flare = Math.min(scaled(Metrics.flare) * stretch, along / 2);

    let x = 0;
    let y = 0;
    switch (edge) {
    case Edge.RIGHT:
        x = width - w;
        y = Math.round((height - h) / 2);
        break;
    case Edge.LEFT:
        y = Math.round((height - h) / 2);
        break;
    case Edge.TOP:
        x = Math.round((width - w) / 2);
        break;
    case Edge.BOTTOM:
    default:
        x = Math.round((width - w) / 2);
        y = height - h;
        break;
    }

    return {
        x, y, width: w, height: h, radius, flare,
        // The marks arrive last, on a shape that has already settled.
        cells: easeOutCubic((progress - Reveal.cellsFrom) / (1 - Reveal.cellsFrom)),
        // The tongue is the collapsed state's only visible part, so it goes
        // as soon as anything else is there to see.
        tongue: 1 - clamp01(progress / Reveal.stretchFrom),
    };
}
