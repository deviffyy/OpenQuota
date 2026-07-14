export interface SpendRingSlice {
  id: string;
  value: number;
}

export interface SpendRingArc {
  id: string;
  start: number;
  end: number;
}

export interface SpendRingGeometry {
  ringDiameter: number;
  innerRadiusRatio: number;
  gapWidth: number;
  cornerRadius: number;
}

export const MINIMUM_SPEND_SLICE_SHARE = 0.025;

export function spendRingArcs(slices: SpendRingSlice[]): SpendRingArc[] {
  const total = slices.reduce((sum, slice) => sum + slice.value, 0);
  if (total <= 0) return [];
  const floored = slices.map((slice) => Math.max(slice.value / total, MINIMUM_SPEND_SLICE_SHARE));
  const flooredTotal = floored.reduce((sum, share) => sum + share, 0);
  let cursor = 0;
  return slices.map((slice, index) => {
    const width = floored[index] / flooredTotal;
    const arc = { id: slice.id, start: cursor, end: cursor + width };
    cursor += width;
    return arc;
  });
}

interface Point {
  x: number;
  y: number;
}

interface SectorLayout {
  center: number;
  outer: number;
  inner: number;
  start: number;
  end: number;
  corner: number;
  betaOuter: number;
  betaInner: number;
}

function sectorLayout(arc: SpendRingArc, geometry: SpendRingGeometry): SectorLayout | null {
  const outer = geometry.ringDiameter / 2;
  const inner = outer * geometry.innerRadiusRatio;
  const top = -Math.PI / 2;
  const halfGap = geometry.gapWidth / outer / 2;
  const start = top + arc.start * Math.PI * 2 + halfGap;
  const end = top + arc.end * Math.PI * 2 - halfGap;
  const width = end - start;
  if (width <= 0.001) return null;

  const sine = Math.sin(Math.min(width / 2, Math.PI / 2));
  let corner = Math.min(geometry.cornerRadius, (outer - inner) / 2);
  corner = Math.min(corner, (outer * sine) / (1 + sine));
  if (sine < 1) corner = Math.min(corner, (inner * sine) / (1 - sine));

  return {
    center: outer,
    outer,
    inner,
    start,
    end,
    corner,
    betaOuter: corner < 0.25 ? 0 : Math.asin(Math.min(1, corner / (outer - corner))),
    betaInner: corner < 0.25 ? 0 : Math.asin(Math.min(1, corner / (inner + corner))),
  };
}

function polar(center: number, radius: number, angle: number): Point {
  return {
    x: center + radius * Math.cos(angle),
    y: center + radius * Math.sin(angle),
  };
}

function coordinate(value: number) {
  return Number(value.toFixed(4));
}

function pointCommand(point: Point) {
  return `${coordinate(point.x)} ${coordinate(point.y)}`;
}

export function ringSectorPath(arc: SpendRingArc, geometry: SpendRingGeometry): string {
  const layout = sectorLayout(arc, geometry);
  if (!layout) return '';
  const { center, outer, inner, start, end, corner, betaOuter, betaInner } = layout;
  const largeArc = end - start > Math.PI ? 1 : 0;

  if (corner < 0.25) {
    return [
      `M ${pointCommand(polar(center, outer, start))}`,
      `A ${outer} ${outer} 0 ${largeArc} 1 ${pointCommand(polar(center, outer, end))}`,
      `L ${pointCommand(polar(center, inner, end))}`,
      `A ${inner} ${inner} 0 ${largeArc} 0 ${pointCommand(polar(center, inner, start))}`,
      'Z',
    ].join(' ');
  }

  const outerStart = polar(center, outer, start + betaOuter);
  const outerEnd = polar(center, outer, end - betaOuter);
  const trailingOuter = polar(center, outer - corner, end);
  const trailingInner = polar(center, inner + corner, end);
  const innerEnd = polar(center, inner, end - betaInner);
  const innerStart = polar(center, inner, start + betaInner);
  const leadingInner = polar(center, inner + corner, start);
  const leadingOuter = polar(center, outer - corner, start);

  return [
    `M ${pointCommand(outerStart)}`,
    `A ${outer} ${outer} 0 ${largeArc} 1 ${pointCommand(outerEnd)}`,
    `Q ${pointCommand(polar(center, outer, end))} ${pointCommand(trailingOuter)}`,
    `L ${pointCommand(trailingInner)}`,
    `Q ${pointCommand(polar(center, inner, end))} ${pointCommand(innerEnd)}`,
    `A ${inner} ${inner} 0 ${largeArc} 0 ${pointCommand(innerStart)}`,
    `Q ${pointCommand(polar(center, inner, start))} ${pointCommand(leadingInner)}`,
    `L ${pointCommand(leadingOuter)}`,
    `Q ${pointCommand(polar(center, outer, start))} ${pointCommand(outerStart)}`,
    'Z',
  ].join(' ');
}

export function fillRingSector(
  context: CanvasRenderingContext2D,
  arc: SpendRingArc,
  geometry: SpendRingGeometry,
  left: number,
  top: number,
) {
  const layout = sectorLayout(arc, geometry);
  if (!layout) return;
  const { center, outer, inner, start, end, corner, betaOuter, betaInner } = layout;
  const at = (radius: number, angle: number) => {
    const point = polar(center, radius, angle);
    return { x: left + point.x, y: top + point.y };
  };

  context.beginPath();
  if (corner < 0.25) {
    const first = at(outer, start);
    context.moveTo(first.x, first.y);
    context.arc(left + center, top + center, outer, start, end, false);
    const innerEnd = at(inner, end);
    context.lineTo(innerEnd.x, innerEnd.y);
    context.arc(left + center, top + center, inner, end, start, true);
    context.closePath();
    context.fill();
    return;
  }

  const first = at(outer, start + betaOuter);
  context.moveTo(first.x, first.y);
  context.arc(left + center, top + center, outer, start + betaOuter, end - betaOuter, false);
  const outerControl = at(outer, end);
  const trailingOuter = at(outer - corner, end);
  context.quadraticCurveTo(outerControl.x, outerControl.y, trailingOuter.x, trailingOuter.y);
  const trailingInner = at(inner + corner, end);
  context.lineTo(trailingInner.x, trailingInner.y);
  const innerControl = at(inner, end);
  const innerEnd = at(inner, end - betaInner);
  context.quadraticCurveTo(innerControl.x, innerControl.y, innerEnd.x, innerEnd.y);
  context.arc(left + center, top + center, inner, end - betaInner, start + betaInner, true);
  const leadingInnerControl = at(inner, start);
  const leadingInner = at(inner + corner, start);
  context.quadraticCurveTo(
    leadingInnerControl.x,
    leadingInnerControl.y,
    leadingInner.x,
    leadingInner.y,
  );
  const leadingOuter = at(outer - corner, start);
  context.lineTo(leadingOuter.x, leadingOuter.y);
  const leadingOuterControl = at(outer, start);
  context.quadraticCurveTo(leadingOuterControl.x, leadingOuterControl.y, first.x, first.y);
  context.closePath();
  context.fill();
}
