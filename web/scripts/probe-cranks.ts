// Probe: what happens when several cranks land in the same block?
const store = new Map<string, string>();
(globalThis as any).localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => store.set(k, v) };
const { openSession } = await import('../src/chain.ts');
const { RPC_URL } = await import('../src/config.ts');
const rpc = async (method: string, params: unknown) =>
  (await (await fetch(RPC_URL, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }) })).json());
const s = await openSession();
await s.ensureFunds();
const first = await s.crank();
await new Promise((r) => setTimeout(r, 3000));
for (let i = 0; i < 5; i++) await s.sendInputs(10 + i, new Uint8Array(4).fill(1));
await new Promise((r) => setTimeout(r, 2500));
const hashes = [first, ...(await Promise.all([1, 2, 3, 4, 5, 6].map(() => s.crank())))];
await new Promise((r) => setTimeout(r, 6000));
for (const h of hashes) {
  const st = await rpc('getTransactionStatus', { transactionHash: h });
  const rc = await rpc('getTransactionReceipt', { transactionHash: h });
  console.log(h.slice(0, 8), JSON.stringify(st.result ?? st.error), JSON.stringify(rc.result ? { block: rc.result.blockHeight, exec: rc.result.execution, gas: rc.result.gasUsed } : rc.error).slice(0, 400));
}
