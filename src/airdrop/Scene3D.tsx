import { Line, OrbitControls } from "@react-three/drei";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { useEffect, useMemo, useRef, type MutableRefObject } from "react";
import * as THREE from "three";
import type { OrbitControls as OrbitControlsImpl } from "three-stdlib";
import { samplePath } from "../lib/format";
import type { Palette } from "../lib/theme";
import type { DropReport, Heightmap, PathPoint, Scenario } from "./types";

// ENU (east, north, up) -> three.js (x right, y up, z towards the viewer).
const toThree = (e: number, n: number, u: number) => new THREE.Vector3(e, u, -n);

/** Models are drawn larger than life so they stay visible at drop-zone scale. */
export const MODEL_SCALE = 6;

export interface Playback {
  t: number;
  playing: boolean;
  speed: number;
  follow: boolean;
}

export const lerpPoint = (a: PathPoint, b: PathPoint, w: number): PathPoint => {
  const mix = (x: number, y: number) => x + (y - x) * w;
  const v3 = (p: PathPoint["pos"], q: PathPoint["pos"]) => ({ x: mix(p.x, q.x), y: mix(p.y, q.y), z: mix(p.z, q.z) });
  return {
    t: mix(a.t, b.t),
    pos: v3(a.pos, b.pos),
    vel: v3(a.vel, b.vel),
    wind: v3(a.wind, b.wind),
    density: mix(a.density, b.density),
    fill: mix(a.fill, b.fill),
  };
};

export function heightAt(hm: Heightmap, x: number, y: number): number {
  const fx = Math.min(Math.max((x - hm.x0) / hm.dx, 0), hm.nx - 1);
  const fy = Math.min(Math.max((y - hm.y0) / hm.dy, 0), hm.ny - 1);
  const i = Math.min(Math.floor(fx), hm.nx - 2);
  const j = Math.min(Math.floor(fy), hm.ny - 2);
  const tx = fx - i;
  const ty = fy - j;
  const h = (a: number, b: number) => hm.heights[b * hm.nx + a];
  return (h(i, j) * (1 - tx) + h(i + 1, j) * tx) * (1 - ty) + (h(i, j + 1) * (1 - tx) + h(i + 1, j + 1) * tx) * ty;
}

/** Text label drawn to a canvas texture; stays a constant size on screen. */
function Label({ position, text, palette }: { position: THREE.Vector3; text: string; palette: Palette }) {
  const sprite = useMemo(() => {
    const scale = 2;
    const font = `${12 * scale}px system-ui, sans-serif`;
    const c = document.createElement("canvas");
    const ctx = c.getContext("2d")!;
    ctx.font = font;
    const w = Math.ceil(ctx.measureText(text).width) + 14 * scale;
    const h = 20 * scale;
    c.width = w;
    c.height = h;
    ctx.font = font;
    ctx.fillStyle = palette.surface;
    ctx.strokeStyle = palette.border;
    ctx.lineWidth = scale;
    ctx.beginPath();
    ctx.roundRect(1, 1, w - 2, h - 2, 4 * scale);
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = palette.text;
    ctx.textBaseline = "middle";
    ctx.fillText(text, 7 * scale, h / 2 + scale);
    const tex = new THREE.CanvasTexture(c);
    tex.colorSpace = THREE.SRGBColorSpace;
    const mat = new THREE.SpriteMaterial({ map: tex, depthTest: false, sizeAttenuation: false });
    const sp = new THREE.Sprite(mat);
    const px = 0.0011; // world-units-per-pixel factor for sizeAttenuation=false sprites
    sp.scale.set((w / scale) * px, (h / scale) * px, 1);
    sp.center.set(-0.08, 0.5);
    sp.renderOrder = 10;
    return sp;
  }, [text, palette]);
  useEffect(
    () => () => {
      sprite.material.map?.dispose();
      sprite.material.dispose();
    },
    [sprite],
  );
  return <primitive object={sprite} position={position} />;
}

function TerrainMesh({ hm, palette }: { hm: Heightmap; palette: Palette }) {
  const geometry = useMemo(() => {
    const g = new THREE.BufferGeometry();
    const pos = new Float32Array(hm.nx * hm.ny * 3);
    for (let j = 0; j < hm.ny; j++)
      for (let i = 0; i < hm.nx; i++) {
        const k = (j * hm.nx + i) * 3;
        pos[k] = hm.x0 + i * hm.dx;
        pos[k + 1] = hm.heights[j * hm.nx + i];
        pos[k + 2] = -(hm.y0 + j * hm.dy);
      }
    const idx: number[] = [];
    for (let j = 0; j < hm.ny - 1; j++)
      for (let i = 0; i < hm.nx - 1; i++) {
        const a = j * hm.nx + i;
        const b = a + 1;
        const c = a + hm.nx;
        const d = c + 1;
        idx.push(a, c, b, b, c, d);
      }
    g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    g.setIndex(idx);
    g.computeVertexNormals();
    return g;
  }, [hm]);
  useEffect(() => () => geometry.dispose(), [geometry]);
  return (
    <group>
      <mesh geometry={geometry} receiveShadow>
        <meshStandardMaterial color={palette.ground} roughness={1} side={THREE.DoubleSide} />
      </mesh>
      <mesh geometry={geometry} position={[0, 0.3, 0]}>
        <meshBasicMaterial color={palette.groundLine} wireframe transparent opacity={0.35} />
      </mesh>
    </group>
  );
}

function circle(cx: number, cy: number, r: number, hm: Heightmap, lift = 0.8, n = 96): THREE.Vector3[] {
  return Array.from({ length: n + 1 }, (_, i) => {
    const a = (i / n) * Math.PI * 2;
    const x = cx + r * Math.cos(a);
    const y = cy + r * Math.sin(a);
    return toThree(x, y, heightAt(hm, x, y) + lift);
  });
}

function Aircraft({ color }: { color: string }) {
  // pointing along +x in local space
  const s = MODEL_SCALE;
  return (
    <group scale={s}>
      <mesh rotation={[0, 0, -Math.PI / 2]}>
        <cylinderGeometry args={[0.9, 0.9, 9, 12]} />
        <meshStandardMaterial color={color} />
      </mesh>
      <mesh position={[0.5, 0.4, 0]}>
        <boxGeometry args={[1.8, 0.2, 13]} />
        <meshStandardMaterial color={color} />
      </mesh>
      <mesh position={[-4, 0.3, 0]}>
        <boxGeometry args={[1.2, 0.15, 4.5]} />
        <meshStandardMaterial color={color} />
      </mesh>
      <mesh position={[-4, 1.4, 0]}>
        <boxGeometry args={[1.2, 2.2, 0.15]} />
        <meshStandardMaterial color={color} />
      </mesh>
    </group>
  );
}

/** Canopy and risers at full inflation; the scene scales it by sqrt(fill). */
function Canopy({ color }: { color: string }) {
  const r = 2.2;
  return (
    <group scale={MODEL_SCALE}>
      <mesh>
        <sphereGeometry args={[r, 24, 12, 0, Math.PI * 2, 0, Math.PI / 2]} />
        <meshStandardMaterial color={color} side={THREE.DoubleSide} transparent opacity={0.92} />
      </mesh>
      {[0, 1, 2, 3].map((k) => {
        const a = (k / 4) * Math.PI * 2 + Math.PI / 4;
        return (
          <Line
            key={k}
            points={[
              [r * Math.cos(a), 0, r * Math.sin(a)],
              [0, -2.8, 0],
            ]}
            color={color}
            lineWidth={1}
          />
        );
      })}
    </group>
  );
}

function WindArrows({ report, palette, offset }: { report: DropReport; palette: Palette; offset: THREE.Vector3 }) {
  const arrows = useMemo(() => {
    const out: THREE.ArrowHelper[] = [];
    report.wind_profile.forEach((w, i) => {
      if (i % 3 !== 0) return;
      const speed = Math.hypot(w.wind.x, w.wind.y);
      if (speed < 0.05) return;
      const dir = toThree(w.wind.x, w.wind.y, 0).normalize();
      const origin = offset.clone().add(new THREE.Vector3(0, w.altitude, 0));
      const len = speed * 8;
      out.push(new THREE.ArrowHelper(dir, origin, len, palette.aqua, Math.min(14, len * 0.35), 7));
    });
    return out;
  }, [report, palette, offset]);
  useEffect(() => () => arrows.forEach((a) => a.dispose()), [arrows]);
  return (
    <>
      {arrows.map((a, i) => (
        <primitive key={i} object={a} />
      ))}
    </>
  );
}

interface SceneProps {
  scenario: Scenario;
  report: DropReport;
  impacts: [number, number][] | null;
  palette: Palette;
  playback: MutableRefObject<Playback>;
  tStart: number;
  tEnd: number;
  onTick: (p: PathPoint, t: number) => void;
  resetView: number;
}

function SceneContent({ scenario, report, impacts, palette, playback, tStart, tEnd, onTick, resetView }: SceneProps) {
  const rel = report.release;
  const path = report.path;
  const hm = report.heightmap;
  const [tx, ty] = scenario.target;
  const groundAtTarget = heightAt(hm, tx, ty);
  const track = (scenario.aircraft.track_deg * Math.PI) / 180;
  const u = { e: Math.sin(track), n: Math.cos(track) };
  const speed = scenario.aircraft.ground_speed;

  const aircraftRef = useRef<THREE.Group>(null);
  const packageRef = useRef<THREE.Group>(null);
  const dropLineRef = useRef<THREE.Group>(null);
  const controls = useRef<OrbitControlsImpl>(null);
  const lastTick = useRef(0);
  const { camera } = useThree();

  const freefall = useMemo(() => path.filter((p) => p.fill < 0.02).map((p) => toThree(p.pos.x, p.pos.y, p.pos.z)), [path]);
  const canopy = useMemo(() => {
    const i = path.findIndex((p) => p.fill >= 0.02);
    return i < 0 ? [] : path.slice(Math.max(0, i - 1)).map((p) => toThree(p.pos.x, p.pos.y, p.pos.z));
  }, [path]);
  const groundTrack = useMemo(
    () => path.map((p) => toThree(p.pos.x, p.pos.y, heightAt(hm, p.pos.x, p.pos.y) + 0.6)),
    [path, hm],
  );

  const trackLen = Math.max(600, rel.release_distance_before_target * 2.5);
  const trackLine = useMemo(() => {
    const [rx, ry] = rel.release_point;
    return [
      toThree(rx - u.e * trackLen, ry - u.n * trackLen, rel.release_altitude),
      toThree(rx + u.e * trackLen, ry + u.n * trackLen, rel.release_altitude),
    ];
  }, [rel, u.e, u.n, trackLen]);

  const dz = useMemo(() => circle(tx, ty, scenario.drop_zone_radius, hm), [tx, ty, scenario.drop_zone_radius, hm]);
  const bullseye = useMemo(() => circle(tx, ty, 12, hm, 1), [tx, ty, hm]);

  const impactGeometry = useMemo(() => {
    if (!impacts) return null;
    const g = new THREE.BufferGeometry();
    const pts = new Float32Array(impacts.length * 3);
    impacts.forEach(([x, y], i) => {
      pts[i * 3] = x;
      pts[i * 3 + 1] = heightAt(hm, x, y) + 1.2;
      pts[i * 3 + 2] = -y;
    });
    g.setAttribute("position", new THREE.BufferAttribute(pts, 3));
    return g;
  }, [impacts, hm]);
  useEffect(() => () => impactGeometry?.dispose(), [impactGeometry]);

  const center = useMemo(
    () => toThree((rel.release_point[0] + tx) / 2, (rel.release_point[1] + ty) / 2, groundAtTarget + (rel.release_altitude - groundAtTarget) * 0.4),
    [rel, tx, ty, groundAtTarget],
  );

  useEffect(() => {
    // Look at the drop from the right of the track so it reads left to right.
    const span = Math.max(450, Math.hypot(rel.release_point[0] - tx, rel.release_point[1] - ty) * 1.25, (rel.release_altitude - groundAtTarget) * 1.6);
    const right = toThree(u.n, -u.e, 0);
    const back = toThree(-u.e, -u.n, 0);
    camera.position.copy(center).addScaledVector(right, span * 1.3).addScaledVector(back, span * 0.15).add(new THREE.Vector3(0, span * 0.3, 0));
    camera.near = 1;
    camera.far = span * 30;
    camera.updateProjectionMatrix();
    controls.current?.target.copy(center);
    controls.current?.update();
    // reset only when the user asks or the geometry changes a lot
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [resetView]);

  const windOffset = useMemo(() => toThree(tx, ty, groundAtTarget).add(new THREE.Vector3(-80, 0, -80)), [tx, ty, groundAtTarget]);

  useFrame((_, delta) => {
    const pb = playback.current;
    if (pb.playing) {
      pb.t += delta * pb.speed;
      if (pb.t > tEnd) pb.t = tStart;
    }
    const t = pb.t;
    // aircraft flies its track the whole time; release happens at t = 0
    const [rx, ry] = rel.release_point;
    const ap = toThree(rx + u.e * speed * t, ry + u.n * speed * t, rel.release_altitude);
    if (aircraftRef.current) {
      aircraftRef.current.position.copy(ap);
      aircraftRef.current.rotation.set(0, Math.PI / 2 - track, 0);
    }
    const p = t <= 0 ? null : samplePath(path, t, lerpPoint);
    if (packageRef.current) {
      packageRef.current.visible = p !== null;
      if (p) packageRef.current.position.copy(toThree(p.pos.x, p.pos.y, p.pos.z));
    }
    if (dropLineRef.current) {
      dropLineRef.current.visible = p !== null;
      if (p) {
        const g = heightAt(hm, p.pos.x, p.pos.y);
        dropLineRef.current.position.copy(toThree(p.pos.x, p.pos.y, g));
        dropLineRef.current.scale.set(1, Math.max(0.01, p.pos.z - g), 1);
      }
    }
    if (pb.follow && controls.current) {
      const focus = p ? toThree(p.pos.x, p.pos.y, p.pos.z) : ap;
      const shift = focus.clone().sub(controls.current.target).multiplyScalar(0.1);
      controls.current.target.add(shift);
      camera.position.add(shift);
    }
    const now = performance.now();
    if (now - lastTick.current > 80) {
      lastTick.current = now;
      onTick(p ?? path[0], t);
    }
  });

  const pkgFill = useRef<THREE.Group>(null);
  useFrame(() => {
    const t = playback.current.t;
    const fill = t <= 0 ? 0 : samplePath(path, t, lerpPoint).fill;
    if (pkgFill.current) {
      const r = Math.sqrt(Math.max(fill, 0.04));
      pkgFill.current.scale.set(r, r, r);
      pkgFill.current.visible = fill > 0.001;
    }
  });

  return (
    <>
      <color attach="background" args={[palette.sky]} />
      <hemisphereLight args={[palette.sky, palette.ground, 1.1]} />
      <directionalLight position={[-400, 900, 300]} intensity={1.6} />
      <TerrainMesh hm={hm} palette={palette} />

      <Line points={dz} color={palette.text2} lineWidth={1.5} dashed dashSize={12} gapSize={8} />
      <Line points={bullseye} color={palette.text} lineWidth={2} />
      <Label position={toThree(tx, ty, groundAtTarget + 2)} text="Target" palette={palette} />

      <Line points={trackLine} color={palette.muted} lineWidth={1} dashed dashSize={20} gapSize={14} />
      <Label position={toThree(rel.release_point[0], rel.release_point[1], rel.release_altitude)} text="Release (CARP)" palette={palette} />

      {freefall.length > 1 && <Line points={freefall} color={palette.orange} lineWidth={3} />}
      {canopy.length > 1 && <Line points={canopy} color={palette.blue} lineWidth={3} />}
      <Line points={groundTrack} color={palette.text2} lineWidth={1} transparent opacity={0.6} />

      <WindArrows report={report} palette={palette} offset={windOffset} />

      {impactGeometry && (
        <points geometry={impactGeometry}>
          <pointsMaterial color={palette.blue} size={3} sizeAttenuation={false} transparent opacity={0.7} />
        </points>
      )}

      <group ref={aircraftRef}>
        <Aircraft color={palette.text2} />
      </group>

      <group ref={packageRef}>
        <group scale={MODEL_SCALE}>
          <mesh>
            <boxGeometry args={[1.2, 0.9, 1.2]} />
            <meshStandardMaterial color={palette.orange} />
          </mesh>
        </group>
        <group ref={pkgFill} position={[0, 3.2 * MODEL_SCALE, 0]} visible={false}>
          <Canopy color={palette.blue} />
        </group>
      </group>

      <group ref={dropLineRef}>
        <mesh position={[0, 0.5, 0]}>
          <boxGeometry args={[0.4, 1, 0.4]} />
          <meshBasicMaterial color={palette.muted} transparent opacity={0.5} />
        </mesh>
      </group>

      <OrbitControls ref={controls} makeDefault enableDamping dampingFactor={0.12} maxPolarAngle={Math.PI * 0.495} />
    </>
  );
}

export function Scene3D(props: SceneProps) {
  return (
    <Canvas camera={{ fov: 45, position: [-600, 500, 800], near: 1, far: 50000 }} dpr={[1, 2]}>
      <SceneContent {...props} />
    </Canvas>
  );
}
