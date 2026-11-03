// Writes to the chain: a throwaway session key per browser, funded by the
// faucet, signing join / send_inputs / crank directly. No backend.

import {
  Ed25519Signer,
  createPubkeyAccount,
  createThruputClient,
  createWalletClient,
  addressFromBase58,
  futurenet,
  gasAmount,
  toBase58,
} from '@thruputchain/client';
import { ArenaServer } from './arena.gen';
import { ARENA_ADDRESS } from './config';
import { bytesToHex, fetchBalance, hexToBytes } from './rpc';

const SEED_KEY = 'playonthruput.session-seed';
const ARENA = addressFromBase58(ARENA_ADDRESS);

// Inner budgets per call; the wallet root wraps each with headroom. The
// heaviest crank measured on RV32 is ~3M gas.
const GAS = {
  join: 200_000n,
  sendInputs: 200_000n,
  crank: 20_000_000n,
};
const WALLET_OVERHEAD = 2_000_000n;
/** Below this many raw units, top up from the faucet (1 token = 1e7). */
const LOW_BALANCE = 100_0000000n;

function loadSeed(): Uint8Array {
  // A game session key, not a wallet holding value: localStorage is fine.
  const stored = localStorage.getItem(SEED_KEY);
  if (stored) return hexToBytes(stored);
  const seed = crypto.getRandomValues(new Uint8Array(32));
  localStorage.setItem(SEED_KEY, bytesToHex(seed));
  return seed;
}

export type Session = Awaited<ReturnType<typeof openSession>>;

export async function openSession() {
  const client = await createThruputClient({ cluster: futurenet() });
  const account = createPubkeyAccount({ signer: Ed25519Signer.fromSeed(loadSeed()) });
  const wallet = createWalletClient({ client, account });
  const address = toBase58(account.address);
  const id = Uint8Array.from(account.address);

  async function send(invocation: Parameters<typeof wallet.execute>[0]['invocation'], inner: bigint): Promise<string> {
    const hash = await wallet.execute({ invocation, gasLimit: gasAmount(inner + WALLET_OVERHEAD) });
    return toBase58(hash);
  }

  return {
    address,
    /** The 32-byte account address the arena knows this player by. */
    id,

    async ensureFunds(): Promise<void> {
      if ((await fetchBalance(address)) >= LOW_BALANCE) return;
      await wallet.requestAirdrop();
    },

    join(): Promise<string> {
      return send(ArenaServer.v0_1.join.invocation(ARENA, gasAmount(GAS.join)), GAS.join);
    },

    sendInputs(startTic: number, buttons: Uint8Array): Promise<string> {
      const invocation = ArenaServer.v0_1.sendInputs.invocation(ARENA, gasAmount(GAS.sendInputs), {
        startTic: BigInt(startTic),
        buttons,
      });
      return send(invocation, GAS.sendInputs);
    },

    crank(): Promise<string> {
      return send(ArenaServer.v0_1.crank.invocation(ARENA, gasAmount(GAS.crank)), GAS.crank);
    },
  };
}
