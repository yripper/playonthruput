// Raw JSON-RPC reads. The public node allows any origin, so the browser talks
// to the chain directly: there is no backend.

import { ARENA_ADDRESS, CHUNK_LEN, RPC_URL, STATE_KEYS } from './config';

interface RpcReply<T> {
  id: number;
  result?: T;
  error?: { code: number; message: string };
}

async function call<T>(requests: { method: string; params: unknown }[]): Promise<T[]> {
  const body = requests.map((r, id) => ({ jsonrpc: '2.0', id, ...r }));
  const res = await fetch(RPC_URL, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  if (!res.ok) throw new Error(`rpc ${res.status}`);
  const replies = (await res.json()) as RpcReply<T>[];
  replies.sort((a, b) => a.id - b.id);
  return replies.map((r) => {
    if (r.error) throw new Error(`rpc: ${r.error.message}`);
    return r.result as T;
  });
}

export function hexToBytes(hex: string): Uint8Array {
  const clean = hex.replace(/^0x/, '');
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

interface StorageItem {
  stateBlockHeight: string;
  item: { kind: string; bytes?: string } | null;
}

export interface ArenaState {
  bytes: Uint8Array;
  height: number;
}

/** The stored arena (all chunks, one round trip), or null before the first crank. */
export async function fetchState(): Promise<ArenaState | null> {
  const items = await call<StorageItem>(
    STATE_KEYS.map((key) => ({ method: 'getStorage', params: { address: ARENA_ADDRESS, key } })),
  );
  // The chunks are separate reads; a crank landing between them would mix two
  // states. Skip the poll rather than decode that.
  if (items.some((it) => it.stateBlockHeight !== items[0].stateBlockHeight)) return null;
  const bytes = new Uint8Array(STATE_KEYS.length * CHUNK_LEN);
  for (const [i, it] of items.entries()) {
    if (!it.item?.bytes) return null;
    bytes.set(hexToBytes(it.item.bytes), i * CHUNK_LEN);
  }
  return { bytes, height: Number(items[0].stateBlockHeight) };
}

/** Chain time minus local time, in ms, from the latest block's timestamp. */
export async function sampleClockOffset(): Promise<number> {
  const [tip] = await call<{ blockHeight: string }>([{ method: 'getChainTip', params: {} }]);
  const sentAt = Date.now();
  const [block] = await call<{ blockTimepoint: string } | null>([
    { method: 'getBlockByHeight', params: { blockHeight: tip.blockHeight } },
  ]);
  if (!block) return 0;
  const chainMs = Number(BigInt(block.blockTimepoint) / 1_000_000n);
  return chainMs - (sentAt + Date.now()) / 2;
}

/**
 * Chain time minus local time, from one of this browser's own transactions:
 * the stamp of the block it landed in minus when it was sent. Null until it
 * has landed. The block a transaction lands in is the clock its effects run on.
 */
export async function inclusionOffset(transactionHash: string, sentAt: number): Promise<number | null> {
  const [status] = await call<{ status: string; blockHeight?: string }>([
    { method: 'getTransactionStatus', params: { transactionHash } },
  ]);
  if (status.status !== 'committed' || !status.blockHeight) return null;
  const [block] = await call<{ blockTimepoint: string } | null>([
    { method: 'getBlockByHeight', params: { blockHeight: status.blockHeight } },
  ]);
  if (!block) return null;
  return Number(BigInt(block.blockTimepoint) / 1_000_000n) - sentAt;
}

export interface RawEvent {
  transactionHash: string;
  blockHeight: string;
  sequenceNumber: number;
  event: { address: string; topics: string[]; data: string };
}

interface EventPage {
  events: RawEvent[];
  nextCursor: string | null;
  latestIndexedBlockHeight: string;
}

/** Arena events from `fromHeight` on (one page). */
export async function fetchEvents(fromHeight: number, limit = 100): Promise<EventPage> {
  const [page] = await call<EventPage>([
    {
      method: 'getEvents',
      params: { filters: [{ addresses: [ARENA_ADDRESS] }], start: { blockHeight: String(fromHeight) }, limit },
    },
  ]);
  return page;
}

export async function requestAirdrop(recipient: string): Promise<void> {
  const nonce = new Uint8Array(8);
  crypto.getRandomValues(nonce);
  await call([{ method: 'faucet_requestAirdrop', params: { recipient, clientNonce: '0x' + bytesToHex(nonce) } }]);
}

export async function fetchBalance(address: string): Promise<bigint> {
  const [r] = await call<{ rawAmount?: string; amount?: string }>([{ method: 'getBalance', params: { address } }]);
  return BigInt(r.rawAmount ?? '0');
}
