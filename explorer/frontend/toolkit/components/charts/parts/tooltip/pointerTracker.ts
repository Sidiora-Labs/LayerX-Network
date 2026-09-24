import * as d3 from 'd3';

export interface Pointer {
  id: number;
  point: [number, number] | null;
  prev: [number, number] | null;
  sourceEvent?: PointerEvent;
}

export interface PointerOptions {
  start?: (tracker: Pointer) => void;
  move?: (tracker: Pointer) => void;
  out?: (tracker: Pointer) => void;
  end?: (tracker: Pointer) => void;
}

export function trackPointer(event: PointerEvent, { start, move, out, end }: PointerOptions): number {
  const tracker: Pointer = {
    id: event.pointerId,
    point: null,
    prev: null,
  };

  const id = event.pointerId;
  const target = event.target as Element;
  tracker.point = d3.pointer(event, target);
  target.setPointerCapture(id);

  const untrack = (sourceEvent: PointerEvent) => {
    tracker.sourceEvent = sourceEvent;
    d3.select(target).on(`.${ id }`, null);
    target.releasePointerCapture(id);
    end?.(tracker);
  };

  d3.select(target)
    .on(`touchstart.${ id }`, (sourceEvent: PointerEvent) => {
      const target = sourceEvent.target as Element;
      const touches = d3.pointers(sourceEvent, target);

      // disable current tracker when entering multi touch mode
      touches.length > 1 && untrack(sourceEvent);
    }, { passive: true })
    .on(`pointerup.${ id } pointercancel.${ id } lostpointercapture.${ id }`, (sourceEvent: PointerEvent) => {
      if (sourceEvent.pointerId !== id) {
        return;
      }

      untrack(sourceEvent);
    }, { passive: true })
    .on(`pointermove.${ id }`, (sourceEvent) => {
      if (sourceEvent.pointerId !== id) {
        return;
      }
      tracker.sourceEvent = sourceEvent;
      tracker.prev = tracker.point;
      tracker.point = d3.pointer(sourceEvent, target);
      move?.(tracker);
    }, { passive: true })
    .on(`pointerout.${ id }`, (e) => {
      if (e.pointerId !== id) {
        return;
      }
      tracker.sourceEvent = e;
      tracker.point = null;
      out?.(tracker);
    }, { passive: true });

  start?.(tracker);

  return id;
}
