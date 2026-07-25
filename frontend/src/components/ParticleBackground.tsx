import { useEffect, useRef } from 'react';

interface Props {
  variant?: number;
}

export function ParticleBackground({ variant = 0 }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    let w = window.innerWidth;
    let h = window.innerHeight;
    const particles: { x: number; y: number; vx: number; vy: number; r: number; alpha: number }[] = [];
    const N = 80;

    canvas.width = w;
    canvas.height = h;

    for (let i = 0; i < N; i++) {
      particles.push({
        x: Math.random() * w,
        y: Math.random() * h,
        vx: (Math.random() - 0.5) * 0.3,
        vy: (Math.random() - 0.5) * 0.3,
        r: 1 + Math.random() * 2,
        alpha: 0.15 + Math.random() * 0.25,
      });
    }

    const colors = [
      ['139,92,246', '245,158,11'],
      ['59,130,246', '139,92,246'],
      ['16,185,129', '59,130,246'],
    ];

    let animId: number;

    const draw = () => {
      ctx.clearRect(0, 0, w, h);

      const [c1, c2] = colors[variant % colors.length];

      for (let i = 0; i < N; i++) {
        const p = particles[i];
        p.x += p.vx;
        p.y += p.vy;
        if (p.x < 0) p.x = w;
        if (p.x > w) p.x = 0;
        if (p.y < 0) p.y = h;
        if (p.y > h) p.y = 0;

        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${c1},${p.alpha})`;
        ctx.fill();

        for (let j = i + 1; j < N; j++) {
          const q = particles[j];
          const dx = p.x - q.x;
          const dy = p.y - q.y;
          const dist = Math.sqrt(dx * dx + dy * dy);
          if (dist < 150) {
            ctx.beginPath();
            ctx.moveTo(p.x, p.y);
            ctx.lineTo(q.x, q.y);
            const lineAlpha = 0.05 * (1 - dist / 150);
            ctx.strokeStyle = `rgba(${c2},${lineAlpha})`;
            ctx.lineWidth = 0.5;
            ctx.stroke();
          }
        }
      }

      animId = requestAnimationFrame(draw);
    };

    const handleResize = () => {
      w = window.innerWidth;
      h = window.innerHeight;
      canvas.width = w;
      canvas.height = h;
      particles.forEach(p => {
        p.x = Math.min(p.x, w);
        p.y = Math.min(p.y, h);
      });
    };

    window.addEventListener('resize', handleResize);
    draw();

    return () => {
      cancelAnimationFrame(animId);
      window.removeEventListener('resize', handleResize);
    };
  }, [variant]);

  return (
    <canvas
      ref={canvasRef}
      className="fixed inset-0 z-0 pointer-events-none opacity-40"
    />
  );
}
