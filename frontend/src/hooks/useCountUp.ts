import { useEffect, useRef, useState } from 'react';

export function useCountUp(target: number, duration = 800) {
  const [value, setValue] = useState(0);
  const prevTarget = useRef(0);
  const frameRef = useRef<number>(0);

  useEffect(() => {
    const prefersReduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (prefersReduced) { setValue(target); return; }

    const start = prevTarget.current;
    const diff = target - start;
    const startTime = performance.now();

    function animate(now: number) {
      const elapsed = now - startTime;
      const progress = Math.min(elapsed / duration, 1);
      const eased = 1 - Math.pow(1 - progress, 3); // ease-out cubic
      setValue(Math.round(start + diff * eased));
      if (progress < 1) frameRef.current = requestAnimationFrame(animate);
    }

    frameRef.current = requestAnimationFrame(animate);
    prevTarget.current = target;

    return () => cancelAnimationFrame(frameRef.current);
  }, [target, duration]);

  return value;
}
