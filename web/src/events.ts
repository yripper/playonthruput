// Kill feed and kill cams, decoded from the arena contract's events.

import { addressFromBase58, createThruputClient, futurenet, toBase58 } from '@thruputchain/client';
import { arena } from './arena.gen';
import { ARENA_ADDRESS } from './config';

export interface FragEvent {
  tic: number;
  killer: string;
  victim: string;
  killerSlot: number;
  victimSlot: number;
  blockHeight: number;
}

export interface KillCamEvent {
  tic: number;
  killerSlot: number;
  victimSlot: number;
  frame: Uint8Array;
  blockHeight: number;
  transactionHash: string;
}

const ADDRESS = addressFromBase58(ARENA_ADDRESS);

export async function createEventFeed(fromHeight: number) {
  const client = await createThruputClient({ cluster: futurenet() });
  let next = BigInt(fromHeight);

  function start() {
    return { order: 'ascending' as const, anchor: { kind: 'atHeight' as const, blockHeight: next } };
  }

  return async function poll(): Promise<{ frags: FragEvent[]; cams: KillCamEvent[] }> {
    const [frags, cams] = await Promise.all([
      client.getContractEvents({ event: arena.v0_1.implementation.Frag, addresses: [ADDRESS], start: start(), limit: 100 }),
      client.getContractEvents({ event: arena.v0_1.implementation.KillCam, addresses: [ADDRESS], start: start(), limit: 100 }),
    ]);
    // Both pages are bounded by what the indexer has; resume after it.
    const indexed = BigInt(frags.latestIndexedBlockHeight);
    if (indexed >= next) next = indexed + 1n;
    return {
      frags: frags.events.map((e) => ({
        tic: Number(e.topics.tic),
        killer: toBase58(e.fields.killer),
        victim: toBase58(e.fields.victim),
        killerSlot: Number(e.fields.killer_slot),
        victimSlot: Number(e.fields.victim_slot),
        blockHeight: Number(e.blockHeight),
      })),
      cams: cams.events.map((e) => ({
        tic: Number(e.topics.tic),
        killerSlot: Number(e.fields.killer_slot),
        victimSlot: Number(e.fields.victim_slot),
        frame: e.fields.frame,
        blockHeight: Number(e.blockHeight),
        transactionHash: toBase58(e.transactionHash),
      })),
    };
  };
}
