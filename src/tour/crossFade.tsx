import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";

/** How long a replaced layer stays mounted: its fade-out (tour.css) plus a frame. */
export const LEAVE_MS = 200;

interface Layer {
  /** Unique per instance, so a layer that comes back while its earlier copy still fades is a new element. */
  uid: number;
  id: string;
  node: ReactNode;
  className: string;
  leaving: boolean;
  /** The first layer is simply there; later ones fade in. */
  initial: boolean;
}

let nextUid = 1;

interface CrossFadeProps {
  /** What is shown: a new id fades the old content out and the new in, the same id just re-renders. */
  id: string;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
}

/**
 * Content that cross-fades when its id changes (CSS only, see tour.css). The outgoing layer stays
 * mounted, inert, until its fade is over, so the box is never empty. The only timer exists while
 * something is leaving, and unmount clears it.
 */
export function CrossFade({ id, children, className = "", style }: CrossFadeProps) {
  const [layers, setLayers] = useState<Layer[]>(() => [{ uid: nextUid++, id, node: children, className, leaving: false, initial: true }]);
  // What the current layer last rendered: the frozen content of a layer that is replaced.
  const latest = useRef({ node: children, className });

  let current = layers;
  if (layers[layers.length - 1].id !== id) {
    current = [
      ...layers.map((layer) => (layer.leaving ? layer : { ...layer, leaving: true, ...latest.current })),
      { uid: nextUid++, id, node: children, className, leaving: false, initial: false },
    ];
    setLayers(current);
  }
  useEffect(() => {
    latest.current = { node: children, className };
  });

  const anyLeaving = current.some((layer) => layer.leaving);
  useEffect(() => {
    if (!anyLeaving) return;
    const timer = setTimeout(() => setLayers((all) => all.filter((layer) => !layer.leaving)), LEAVE_MS);
    return () => clearTimeout(timer);
  }, [layers, anyLeaving]);

  return (
    <>
      {current.map((layer, index) => {
        const isCurrent = index === current.length - 1;
        const motion = layer.leaving ? "tour-layer--out" : layer.initial ? "" : "tour-layer--in";
        return (
          <div
            key={layer.uid}
            data-layer-id={layer.id}
            className={`tour-layer ${motion} ${isCurrent ? className : layer.className}`}
            style={{ ...style, pointerEvents: "none" }}
            aria-hidden={isCurrent ? undefined : true}
          >
            {isCurrent ? children : layer.node}
          </div>
        );
      })}
    </>
  );
}
