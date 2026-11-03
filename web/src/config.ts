// Where the arena lives. Everything else is read from the chain at runtime.

export const RPC_URL = 'https://rpc.thruput.network';
export const ARENA_ADDRESS = 'HXWNvZQfn4YnALC83kXWNd9ccu8ytjFvcfv3n8kKuh3Y';

/** Storage keys of the three state chunks (`state_keys()` on the contract). */
export const STATE_KEYS = [
  '0xd7445e49f26a24cf76805d09134fd6d201ab1b9256a75610e69edf8e941213a4',
  '0x3c3b13a3daa161a7057112bad4a54eeb476d7cfc917e8d66f55c39ccb4864e38',
  '0xce98848129d1dbb0db2ff07e8dab88693fbaa9b592fb767ae2e67593743e61d2',
];
export const CHUNK_LEN = 4096;

/** `MAX_BATCH` on the contract: buttons per `send_inputs`. */
export const MAX_BATCH = 218;
