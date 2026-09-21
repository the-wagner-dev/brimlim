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
    revealMs: 180,
    collapseMs: 160,
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
 * Where the container sits on its monitor, and how far the pill travels to
 * hide. The container is anchored so that an un-translated pill sits flush
 * against the edge.
 */
export function placement(edge, monitor, [width, height]) {
    switch (edge) {
    case Edge.LEFT:
        return {
            x: monitor.x,
            y: monitor.y + Math.round((monitor.height - height) / 2),
            hiddenOffset: [-width, 0],
        };
    case Edge.RIGHT:
        return {
            x: monitor.x + monitor.width - width,
            y: monitor.y + Math.round((monitor.height - height) / 2),
            hiddenOffset: [width, 0],
        };
    case Edge.TOP:
        return {
            x: monitor.x + Math.round((monitor.width - width) / 2),
            y: monitor.y,
            hiddenOffset: [0, -height],
        };
    case Edge.BOTTOM:
    default:
        return {
            x: monitor.x + Math.round((monitor.width - width) / 2),
            y: monitor.y + monitor.height - height,
            hiddenOffset: [0, height],
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
