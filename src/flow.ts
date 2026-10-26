// perlin noise flow over a turning sphere, drawn as particle trails

const TAU = Math.PI * 2;
// hue, saturation, lightness; ochre twice as often
const PALETTE = [[38, 64, 58], [18, 55, 52], [40, 56, 80], [87, 19, 62], [16, 59, 45], [38, 64, 58]];
const COUNT = 520;
const TRAIL = 18;
// sim seconds between trail points
const STEP = 0.05;
// sphere radii per sim second
const SPEED = 1.1;
const FREQ = 1.4;
const EVOLVE = 0.25;
// radians per sim second, about an axis tipped toward the viewer
const SPIN = 0.6;
const TILT = 0.4;
// sim time runs this fast in silence and at full voice
const IDLE = 0.2;
const LOUD = 1.8;
// size and saturation gain at full voice
const SWELL = 0.07;
const VIVID = 0.5;
// top left, in front; screen y points down
const LIGHT = [-0.45, -0.55, 0.7];

// ken perlin's improved noise
const perm = new Uint8Array(512);
for (let i = 0; i < 256; i++) perm[i] = i;
for (let i = 255; i > 0; i--) {
  const j = Math.floor(Math.random() * (i + 1));
  [perm[i], perm[j]] = [perm[j], perm[i]];
}
perm.copyWithin(256, 0, 256);

const fade = (t: number) => t * t * t * (t * (t * 6 - 15) + 10);
const lerp = (t: number, a: number, b: number) => a + t * (b - a);

function grad(hash: number, x: number, y: number, z: number) {
  const h = hash & 15;
  const u = h < 8 ? x : y;
  const v = h < 4 ? y : h === 12 || h === 14 ? x : z;
  return (h & 1 ? -u : u) + (h & 2 ? -v : v);
}

function noise(x: number, y: number, z: number) {
  const fx = Math.floor(x), fy = Math.floor(y), fz = Math.floor(z);
  const X = fx & 255, Y = fy & 255, Z = fz & 255;
  x -= fx;
  y -= fy;
  z -= fz;
  const u = fade(x), v = fade(y), w = fade(z);
  const A = perm[X] + Y, AA = perm[A] + Z, AB = perm[A + 1] + Z;
  const B = perm[X + 1] + Y, BA = perm[B] + Z, BB = perm[B + 1] + Z;
  return lerp(
    w,
    lerp(
      v,
      lerp(u, grad(perm[AA], x, y, z), grad(perm[BA], x - 1, y, z)),
      lerp(u, grad(perm[AB], x, y - 1, z), grad(perm[BB], x - 1, y - 1, z)),
    ),
    lerp(
      v,
      lerp(u, grad(perm[AA + 1], x, y, z - 1), grad(perm[BA + 1], x - 1, y, z - 1)),
      lerp(u, grad(perm[AB + 1], x, y - 1, z - 1), grad(perm[BB + 1], x - 1, y - 1, z - 1)),
    ),
  );
}

type Particle = { x: number; y: number; z: number; trail: number[]; age: number; life: number; tone: number };

function spawn(p: Particle) {
  p.z = Math.random() * 2 - 1;
  const r = Math.sqrt(1 - p.z * p.z);
  const a = Math.random() * TAU;
  p.x = r * Math.cos(a);
  p.y = r * Math.sin(a);
  p.trail = [p.x, p.y, p.z];
  p.age = 0;
  p.life = 2 + Math.random() * 3;
}

/** Returns a frame function taking real seconds since the last frame and voice energy 0 to 1. */
export function flow(canvas: HTMLCanvasElement) {
  const ctx = canvas.getContext("2d")!;
  const speckle = document.createElement("canvas");
  speckle.width = speckle.height = 64;
  const sctx = speckle.getContext("2d")!;
  const noiseImg = sctx.createImageData(64, 64);
  for (let i = 3; i < noiseImg.data.length; i += 4) noiseImg.data[i] = Math.random() * 255;
  sctx.putImageData(noiseImg, 0, 0);
  const grain = ctx.createPattern(speckle, "repeat")!;
  const particles: Particle[] = [];
  for (let i = 0; i < COUNT; i++) {
    const p = { tone: i % PALETTE.length } as Particle;
    spawn(p);
    p.age = Math.random() * p.life;
    particles.push(p);
  }
  let t = 0;
  let spin = 0;
  let sinceStep = 0;
  let speed = IDLE;
  let pulse = 0;

  function advance(ds: number) {
    t += ds * EVOLVE;
    spin += ds * SPIN;
    const e = 0.02;
    for (const p of particles) {
      const qx = p.x * FREQ, qy = p.y * FREQ, qz = p.z * FREQ + t;
      const n = noise(qx, qy, qz);
      const gx = noise(qx + e, qy, qz) - n, gy = noise(qx, qy + e, qz) - n, gz = noise(qx, qy, qz + e) - n;
      // position cross gradient runs along the noise contours, so strands never bunch up
      const vx = p.y * gz - p.z * gy, vy = p.z * gx - p.x * gz, vz = p.x * gy - p.y * gx;
      const m = Math.hypot(vx, vy, vz);
      if (m > 0) {
        // calm only near the noise extrema
        const k = (SPEED * ds * Math.min(1, m / e / 0.8)) / m;
        p.x += vx * k;
        p.y += vy * k;
        p.z += vz * k;
        const r = Math.hypot(p.x, p.y, p.z);
        p.x /= r;
        p.y /= r;
        p.z /= r;
      }
      p.age += ds;
      if (p.age > p.life) spawn(p);
    }
    sinceStep += ds;
    for (; sinceStep >= STEP; sinceStep -= STEP) {
      for (const p of particles) {
        p.trail.push(p.x, p.y, p.z);
        if (p.trail.length > TRAIL * 3) p.trail.splice(0, 3);
      }
    }
  }

  function draw() {
    const px = Math.round(canvas.clientWidth * devicePixelRatio);
    if (canvas.width !== px) canvas.width = canvas.height = px;
    const c = px / 2;
    const R = c * 0.86 * (1 + SWELL * pulse);
    const colors = PALETTE.map(([h, s, l]) => `hsl(${h},${Math.min(100, s * (1 + VIVID * pulse))}%,${l}%)`);
    const cs = Math.cos(spin), sn = Math.sin(spin), ct = Math.cos(TILT), st = Math.sin(TILT);
    let X = 0, Y = 0, Z = 0;
    // sphere to view: spin about the y axis, then tip the top toward the viewer
    const view = (x: number, y: number, z: number) => {
      const z1 = cs * z - sn * x;
      X = cs * x + sn * z;
      Y = ct * y + st * z1;
      Z = ct * z1 - st * y;
    };
    const glow = 0.75 + (0.25 * (speed - IDLE)) / (LOUD - IDLE);
    // full trails drop their oldest point every step; slide the tail toward the next one so it never jumps
    const slide = sinceStep / STEP;
    ctx.clearRect(0, 0, px, px);
    // faint body, darker away from the light, fading out at the rim
    const shade = ctx.createRadialGradient(c - 0.35 * R, c - 0.4 * R, 0, c, c, R);
    shade.addColorStop(0, "rgba(40,30,20,0.55)");
    shade.addColorStop(0.85, "rgba(14,10,6,0.85)");
    shade.addColorStop(1, "rgba(14,10,6,0)");
    ctx.globalAlpha = 1;
    ctx.fillStyle = shade;
    ctx.fillRect(0, 0, px, px);
    ctx.lineCap = ctx.lineJoin = "round";
    // far side first, faint, so the near side reads as in front
    for (const near of [false, true]) {
      ctx.lineWidth = (near ? 1.3 : 0.8) * devicePixelRatio;
      for (const p of particles) {
        view(p.x, p.y, p.z);
        if (Z >= 0 !== near) continue;
        const [hx, hy] = [X, Y];
        const lit = Math.max(0, X * LIGHT[0] + Y * LIGHT[1] + Z * LIGHT[2]);
        const fade = Math.min(1, p.age / 0.5, (p.life - p.age) / 0.5);
        ctx.globalAlpha = fade * (near ? glow * (0.5 + 0.5 * lit) : 0.18);
        ctx.strokeStyle = colors[p.tone];
        ctx.beginPath();
        const tr = p.trail;
        let i = 0;
        if (tr.length === TRAIL * 3) {
          view(lerp(slide, tr[0], tr[3]), lerp(slide, tr[1], tr[4]), lerp(slide, tr[2], tr[5]));
          ctx.lineTo(c + X * R, c + Y * R);
          i = 3;
        }
        for (; i < tr.length; i += 3) {
          view(tr[i], tr[i + 1], tr[i + 2]);
          ctx.lineTo(c + X * R, c + Y * R);
        }
        ctx.lineTo(c + hx * R, c + hy * R);
        ctx.stroke();
      }
    }
    // grain: punch fine random holes in the strands
    ctx.globalCompositeOperation = "destination-out";
    ctx.globalAlpha = 0.25;
    ctx.fillStyle = grain;
    ctx.fillRect(0, 0, px, px);
    ctx.globalCompositeOperation = "source-over";
  }

  // fill the trails before the first frame
  for (let i = 0; i < 100; i++) advance(STEP / 2);

  let unseen = 0;
  return (dt: number, energy: number) => {
    // a stalled frame must not jump the sim
    dt = Math.min(dt, 0.05);
    const goal = IDLE + (LOUD - IDLE) * energy;
    // quick to speed up, slow to calm down
    speed += (goal - speed) * (1 - Math.exp(-dt / (goal > speed ? 0.12 : 0.9)));
    // fast enough to follow syllables
    pulse += (energy - pulse) * (1 - Math.exp(-dt / (energy > pulse ? 0.05 : 0.25)));
    unseen += dt;
    // slow motion looks the same at 30 fps, for half the cost
    if (speed < 0.3 && unseen < 1 / 30) return;
    advance(unseen * speed);
    unseen = 0;
    draw();
  };
}
