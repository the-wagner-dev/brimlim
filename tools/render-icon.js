#!/usr/bin/gjs -m
// The application icon, drawn with the product's own Cairo code rather than
// authored separately — a pill with one ring in it, at icon scale.
//
//   gjs -m tools/render-icon.js assets/brimlim.png 256

import Cairo from 'gi://cairo';

import {Edge} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/edges.js';
import {drawMark} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/paint.js';
import {Colors, setColor, usageColor} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/palette.js';
import {drawPill} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/pill.js';

const [outPath = 'assets/brimlim.png', sizeArg = '256'] = ARGV;
const size = Number(sizeArg);
const scale = size / 64;

const surface = new Cairo.ImageSurface(Cairo.Format.ARGB32, size, size);
const cr = new Cairo.Context(surface);

const pillW = Math.round(46 * scale);
const pillH = Math.round(58 * scale);
// Inset a little so the inverted corners are part of the icon rather than
// running off its edge.
const x = size - pillW - Math.round(5 * scale);
const y = Math.round((size - pillH) / 2);

cr.translate(x, y);
drawPill(cr, Edge.RIGHT, pillW, pillH,
    {flare: Math.round(10 * scale), radius: Math.round(14 * scale)});

// The ring is drawn here rather than through paintCell: an icon wants the
// mark and the arc, not the percentage label that belongs beside them.
const ring = Math.round(34 * scale);
const stroke = Math.round(4 * scale);
const cx = Math.round(6 * scale) + ring / 2;
const cy = Math.round((pillH - ring) / 2) + ring / 2;
const radius = ring / 2 - stroke / 2;
const percent = 0.62;

setColor(cr, Colors.disc);
cr.newPath();
cr.arc(cx, cy, radius - stroke / 2 - Math.round(2 * scale), 0, 2 * Math.PI);
cr.fill();

cr.setLineCap(1);
cr.setLineWidth(stroke);

setColor(cr, Colors.track);
cr.newPath();
cr.arc(cx, cy, radius, 0, 2 * Math.PI);
cr.stroke();

setColor(cr, usageColor(percent));
cr.newPath();
cr.arc(cx, cy, radius, -Math.PI / 2, -Math.PI / 2 + 2 * Math.PI * percent);
cr.stroke();

drawMark(cr, 'claude', cx, cy, radius * 0.42, 1);

cr.$dispose();
surface.flush();
surface.writeToPNG(outPath);
print(`wrote ${outPath} (${size}x${size})`);
