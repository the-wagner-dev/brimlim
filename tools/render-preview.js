#!/usr/bin/gjs -m
// Renders the notch's Cairo drawing outside a running Shell, so the shapes
// can be reviewed without installing anything. Uses the extension's own
// geometry, so what you see is what it lays out.
//
//   gjs -m tools/render-preview.js out.png [scale]

import Cairo from 'gi://cairo';

import {Edge} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/edges.js';
import {Metrics, cellSize, pillSize, ringOrigin, scaled, setScale}
    from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/geometry.js';
import {paintCell} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/paint.js';
import {drawPill, drawTongue} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/pill.js';

const [outPath = 'preview.png', scaleArg = '1'] = ARGV;
const scale = Number(scaleArg);
setScale(scale);

const CASES = [
    {id: 'claude', label: 'Claude', headline_percent: 0.73, fidelity: 'official', status: 'ok', activity: 'busy'},
    {id: 'codex', label: 'Codex', headline_percent: 0.21, fidelity: 'official', status: 'ok', activity: 'idle'},
    {id: 'gemini', label: 'Gemini', headline_percent: 0.52, fidelity: 'official', status: 'ok', activity: 'waiting'},
    {id: 'derived', label: 'Derived', headline_percent: 0.94, fidelity: 'derived', status: 'stale', activity: 'idle'},
    {id: 'none', label: 'None', headline_percent: null, fidelity: 'official', status: 'needs_auth', activity: 'idle'},
];

const EDGES = [Edge.RIGHT, Edge.LEFT, Edge.TOP, Edge.BOTTOM];

const margin = scaled(28);
// A gutter between the quadrants: without it the right-edge panel of one
// quadrant butts straight against the left-edge panel of the next and the
// two read as a single pill.
const gutter = scaled(56);
const [, verticalAlong] = pillSize(Edge.RIGHT, CASES.length);
const cellW = verticalAlong + 2 * margin;
const cellH = verticalAlong + 2 * margin;
const width = cellW * 2 + gutter;
const height = cellH * 2 + gutter;

const surface = new Cairo.ImageSurface(Cairo.Format.ARGB32, width, height);
const cr = new Cairo.Context(surface);

// A light desktop behind it, to check the pill reads on a real background.
cr.setSourceRGB(0.83, 0.85, 0.88);
cr.paint();

function drawPanel(edge, originX, originY) {
    const [w, h] = pillSize(edge, CASES.length);

    // Anchor each panel against the cell wall its edge belongs to, the way
    // the notch anchors against the monitor.
    let x = originX;
    let y = originY;
    switch (edge) {
    case Edge.RIGHT: x += cellW - w; y += Math.round((cellH - h) / 2); break;
    case Edge.LEFT: y += Math.round((cellH - h) / 2); break;
    case Edge.TOP: x += Math.round((cellW - w) / 2); break;
    case Edge.BOTTOM: x += Math.round((cellW - w) / 2); y += cellH - h; break;
    }

    cr.save();
    cr.translate(x, y);
    drawPill(cr, edge, w, h, {flare: scaled(Metrics.flare), radius: scaled(Metrics.pillRadius)});

    CASES.forEach((provider, index) => {
        const [rx, ry] = ringOrigin(edge, index);
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
    cr.restore();

    // The resting tongue for this edge, drawn where it would actually sit.
    const thickness = scaled(Metrics.tongueThickness);
    const length = scaled(Metrics.tongueLength);
    cr.save();
    switch (edge) {
    case Edge.RIGHT: cr.translate(originX + cellW - thickness, originY + Math.round((cellH - length) / 2)); break;
    case Edge.LEFT: cr.translate(originX, originY + Math.round((cellH - length) / 2)); break;
    case Edge.TOP: cr.translate(originX + Math.round((cellW - length) / 2), originY); break;
    case Edge.BOTTOM: cr.translate(originX + Math.round((cellW - length) / 2), originY + cellH - thickness); break;
    }
    const vertical = edge === Edge.RIGHT || edge === Edge.LEFT;
    drawTongue(cr, edge, vertical ? thickness : length, vertical ? length : thickness);
    cr.restore();
}

EDGES.forEach((edge, index) => {
    drawPanel(edge,
        (index % 2) * (cellW + gutter),
        Math.floor(index / 2) * (cellH + gutter));
});

cr.$dispose();
surface.flush();
surface.writeToPNG(outPath);
print(`wrote ${outPath} (${width}x${height}, scale ${scale}, cell ${cellSize()})`);
