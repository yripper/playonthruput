// Node smoke test of the browser's chain path against Futurenet.
const store = new Map<string, string>();
(globalThis as any).localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => store.set(k, v) };
const { openSession } = await import('../src/chain.ts');
const { fetchState, sampleClockOffset } = await import('../src/rpc.ts');
const s = await openSession();
console.log('session', s.address);
let t = Date.now();
await s.ensureFunds();
console.log('funded in', Date.now() - t, 'ms');
t = Date.now();
console.log('join tx', await s.join(), Date.now() - t, 'ms');
console.log('clock offset ms', Math.round(await sampleClockOffset()));
const before = await fetchState();
console.log('state height', before?.height);
console.log('inputs tx', await s.sendInputs(10, new Uint8Array(200).fill(1)));
await new Promise((r) => setTimeout(r, 2500));
t = Date.now();
console.log('crank tx', await s.crank(), Date.now() - t, 'ms');
