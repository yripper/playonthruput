// Generated from arena.spec.xdr - ArenaServer interface v0.1.0
// DO NOT EDIT. Re-run codegen to update.

import * as __gh_protocol from "@thruputchain/protocol";
import * as __gh_protocol_utils from "@thruputchain/protocol/utils";
import * as __gh_contract_spec from "@thruputchain/contract-spec";
import * as __gh_client_contract from "@thruputchain/client/contract";

const __gh_s = __gh_contract_spec.contractSpecString;

const __gh_entrypoints = {
  ArenaServer: {
    v0_1: {
      crank: __gh_protocol.contractEntrypointID(__gh_protocol_utils.fromHex("3511f255cdca1250")),
      join: __gh_protocol.contractEntrypointID(__gh_protocol_utils.fromHex("72c0b094663076be")),
      sendInputs: __gh_protocol.contractEntrypointID(__gh_protocol_utils.fromHex("374bc8ff69bde3c2")),
      stateKeys: __gh_protocol.contractEntrypointID(__gh_protocol_utils.fromHex("0c8f91f09f8dc638")),
      stateLen: __gh_protocol.contractEntrypointID(__gh_protocol_utils.fromHex("91269827a5627556")),
      stateSlice: __gh_protocol.contractEntrypointID(__gh_protocol_utils.fromHex("38a64b01992e59e5")),
    },
  },
} as const;

export namespace arena {

  export namespace v0_1 {

    export namespace implementation {

      export type ArenaError = 1n | 2n;

      export interface CrankReport { "tic": bigint; "entries": bigint; "kills": bigint; }

      export interface ArenaOpened extends __gh_client_contract.DecodedContractEvent<
        "ArenaOpened",
        {  },
        { "genesis_ns": bigint; "seed": bigint }
      > {}

      export type ArenaOpenedFilter = globalThis.Record<string, never>;

      export interface Frag extends __gh_client_contract.DecodedContractEvent<
        "Frag",
        { "tic": bigint },
        { "killer": __gh_protocol.ContractAddress; "victim": __gh_protocol.ContractAddress; "killer_slot": bigint; "victim_slot": bigint }
      > {}

      export interface FragFilter {
        "tic"?: __gh_client_contract.ContractEventFilterValue<bigint>;
      }

      export interface KillCam extends __gh_client_contract.DecodedContractEvent<
        "KillCam",
        { "tic": bigint },
        { "killer_slot": bigint; "victim_slot": bigint; "frame": globalThis.Uint8Array }
      > {}

      export interface KillCamFilter {
        "tic"?: __gh_client_contract.ContractEventFilterValue<bigint>;
      }
    }
  }
}

export namespace ArenaServer {

  export namespace v0_1 {

    export type CrankArgs = void;
    export type CrankResult = __gh_codegen.arena$v0_1$implementation$CrankReport;

    export type JoinArgs = void;
    export type JoinResult = void;

    export interface SendInputsArgs {
      startTic: bigint;
      buttons: globalThis.Uint8Array;
    }
    export type SendInputsResult = void;

    export type StateKeysArgs = void;
    export type StateKeysResult = __gh_client_contract.FixedBytes<32>[];

    export type StateLenArgs = void;
    export type StateLenResult = bigint;

    export interface StateSliceArgs {
      offset: bigint;
    }
    export type StateSliceResult = globalThis.Uint8Array;

    export type DeclaredException =
      {
        kind: "declared";
        name: "ArenaError";
        selector: 0x49495a0411040b2dn;
        value: __gh_codegen.arena$v0_1$implementation$ArenaError;
      };
    export type Exception = __gh_client_contract.ContractException<DeclaredException>;
  }
}

export const spec: __gh_client_contract.ContractSpec = {
  interfaces: [
    {
      name: __gh_s("ArenaServer"),
      vers: __gh_s("0.1.0"),
      throws: [
        { selector: 0x49495a0411040b2dn, ty: { v: __gh_contract_spec.ContractTypeKind.Named, named: { crate_name: __gh_s("arena"), major: __gh_s("0.1"), module_path: __gh_s("arena::implementation"), declared: __gh_s("ArenaError") } } },
      ],
      methods: [
        {
          name: __gh_s("crank"),
          args: [
          ],
          return_type: { v: __gh_contract_spec.ContractTypeKind.Named, named: { crate_name: __gh_s("arena"), major: __gh_s("0.1"), module_path: __gh_s("arena::implementation"), declared: __gh_s("CrankReport") } },
          effect: null,
        },
        {
          name: __gh_s("join"),
          args: [
          ],
          return_type: { v: __gh_contract_spec.ContractTypeKind.Void },
          effect: null,
        },
        {
          name: __gh_s("send_inputs"),
          args: [
            { name: __gh_s("start_tic"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
            { name: __gh_s("buttons"), ty: { v: __gh_contract_spec.ContractTypeKind.Vector, vector: { element: { v: __gh_contract_spec.ContractTypeKind.U8 }, max_len: 4294967295n } } },
          ],
          return_type: { v: __gh_contract_spec.ContractTypeKind.Void },
          effect: null,
        },
        {
          name: __gh_s("state_keys"),
          args: [
          ],
          return_type: { v: __gh_contract_spec.ContractTypeKind.Vector, vector: { element: { v: __gh_contract_spec.ContractTypeKind.Array, array: { element: { v: __gh_contract_spec.ContractTypeKind.U8 }, len: 32n } }, max_len: 4294967295n } },
          effect: null,
        },
        {
          name: __gh_s("state_len"),
          args: [
          ],
          return_type: { v: __gh_contract_spec.ContractTypeKind.U32 },
          effect: null,
        },
        {
          name: __gh_s("state_slice"),
          args: [
            { name: __gh_s("offset"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
          ],
          return_type: { v: __gh_contract_spec.ContractTypeKind.Vector, vector: { element: { v: __gh_contract_spec.ContractTypeKind.U8 }, max_len: 4294967295n } },
          effect: null,
        },
      ],
    },
  ],
  type_defs: [
    {
      v: __gh_contract_spec.ContractTypeDefKind.TypeDefEnum,
      enum_def: {
        name: { crate_name: __gh_s("arena"), major: __gh_s("0.1"), module_path: __gh_s("arena::implementation"), declared: __gh_s("ArenaError") },
        variants: [
          { name: __gh_s("EmptyBatch"), value: 1n },
          { name: __gh_s("BatchTooLong"), value: 2n },
        ],
      },
    },
    {
      v: __gh_contract_spec.ContractTypeDefKind.TypeDefStruct,
      struct_def: {
        name: { crate_name: __gh_s("arena"), major: __gh_s("0.1"), module_path: __gh_s("arena::implementation"), declared: __gh_s("CrankReport") },
        fields: [
          { name: __gh_s("tic"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
          { name: __gh_s("entries"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
          { name: __gh_s("kills"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
        ],
      },
    },
  ],
  events: [
    {
      name: { crate_name: __gh_s("arena"), major: __gh_s("0.1"), module_path: __gh_s("arena::implementation"), declared: __gh_s("ArenaOpened") },
      signature: __gh_protocol.topic(__gh_protocol_utils.fromHex("a566c93b6b13c0b0cd3f278365d169e5ce982c9fc8f9c5f5d28de895ef009fb9")),
      topics: [
      ],
      fields: [
        { name: __gh_s("genesis_ns"), ty: { v: __gh_contract_spec.ContractTypeKind.U64 } },
        { name: __gh_s("seed"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
      ],
    },
    {
      name: { crate_name: __gh_s("arena"), major: __gh_s("0.1"), module_path: __gh_s("arena::implementation"), declared: __gh_s("Frag") },
      signature: __gh_protocol.topic(__gh_protocol_utils.fromHex("b06b95e0cc56c3f3a564777cbca5533938571a0c8248517714d8099eecf48652")),
      topics: [
        { name: __gh_s("tic"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
      ],
      fields: [
        { name: __gh_s("killer"), ty: { v: __gh_contract_spec.ContractTypeKind.Address } },
        { name: __gh_s("victim"), ty: { v: __gh_contract_spec.ContractTypeKind.Address } },
        { name: __gh_s("killer_slot"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
        { name: __gh_s("victim_slot"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
      ],
    },
    {
      name: { crate_name: __gh_s("arena"), major: __gh_s("0.1"), module_path: __gh_s("arena::implementation"), declared: __gh_s("KillCam") },
      signature: __gh_protocol.topic(__gh_protocol_utils.fromHex("31defdae4d03eb1b3d1e6d9ebb5633cce15307800b9a05417b2f49c8dee84a0e")),
      topics: [
        { name: __gh_s("tic"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
      ],
      fields: [
        { name: __gh_s("killer_slot"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
        { name: __gh_s("victim_slot"), ty: { v: __gh_contract_spec.ContractTypeKind.U32 } },
        { name: __gh_s("frame"), ty: { v: __gh_contract_spec.ContractTypeKind.Vector, vector: { element: { v: __gh_contract_spec.ContractTypeKind.U8 }, max_len: 4294967295n } } },
      ],
    },
  ],
};

const __gh_codecs = __gh_client_contract.createContractCodecContext(spec.type_defs);

export const ArenaServer = {
  v0_1: {
    crank: __gh_client_contract.defineContractMethod<
      ArenaServer.v0_1.CrankArgs,
      ArenaServer.v0_1.CrankResult,
      ArenaServer.v0_1.DeclaredException
    >({
      interfaceName: "ArenaServer",
      interfaceVersion: "0.1.0",
      abiName: "crank",
      publicName: "crank",
      entrypoint: __gh_entrypoints.ArenaServer.v0_1.crank,
      effect: spec.interfaces[0]!.methods[0]!.effect,
      args: [
      ],
      returnType: spec.interfaces[0]!.methods[0]!.return_type,
      throws: [
        {
          selector: 0x49495a0411040b2dn,
          name: "ArenaError",
          type: spec.interfaces[0]!.throws[0]!.ty,
        },
      ],
      codecs: __gh_codecs,
    }),
    join: __gh_client_contract.defineContractMethod<
      ArenaServer.v0_1.JoinArgs,
      ArenaServer.v0_1.JoinResult,
      ArenaServer.v0_1.DeclaredException
    >({
      interfaceName: "ArenaServer",
      interfaceVersion: "0.1.0",
      abiName: "join",
      publicName: "join",
      entrypoint: __gh_entrypoints.ArenaServer.v0_1.join,
      effect: spec.interfaces[0]!.methods[1]!.effect,
      args: [
      ],
      returnType: spec.interfaces[0]!.methods[1]!.return_type,
      throws: [
        {
          selector: 0x49495a0411040b2dn,
          name: "ArenaError",
          type: spec.interfaces[0]!.throws[0]!.ty,
        },
      ],
      codecs: __gh_codecs,
    }),
    sendInputs: __gh_client_contract.defineContractMethod<
      ArenaServer.v0_1.SendInputsArgs,
      ArenaServer.v0_1.SendInputsResult,
      ArenaServer.v0_1.DeclaredException
    >({
      interfaceName: "ArenaServer",
      interfaceVersion: "0.1.0",
      abiName: "send_inputs",
      publicName: "sendInputs",
      entrypoint: __gh_entrypoints.ArenaServer.v0_1.sendInputs,
      effect: spec.interfaces[0]!.methods[2]!.effect,
      args: [
        {
          abiName: "start_tic",
          publicName: "startTic",
          type: spec.interfaces[0]!.methods[2]!.args[0]!.ty,
        },
        {
          abiName: "buttons",
          publicName: "buttons",
          type: spec.interfaces[0]!.methods[2]!.args[1]!.ty,
        },
      ],
      returnType: spec.interfaces[0]!.methods[2]!.return_type,
      throws: [
        {
          selector: 0x49495a0411040b2dn,
          name: "ArenaError",
          type: spec.interfaces[0]!.throws[0]!.ty,
        },
      ],
      codecs: __gh_codecs,
    }),
    stateKeys: __gh_client_contract.defineContractMethod<
      ArenaServer.v0_1.StateKeysArgs,
      ArenaServer.v0_1.StateKeysResult,
      ArenaServer.v0_1.DeclaredException
    >({
      interfaceName: "ArenaServer",
      interfaceVersion: "0.1.0",
      abiName: "state_keys",
      publicName: "stateKeys",
      entrypoint: __gh_entrypoints.ArenaServer.v0_1.stateKeys,
      effect: spec.interfaces[0]!.methods[3]!.effect,
      args: [
      ],
      returnType: spec.interfaces[0]!.methods[3]!.return_type,
      throws: [
        {
          selector: 0x49495a0411040b2dn,
          name: "ArenaError",
          type: spec.interfaces[0]!.throws[0]!.ty,
        },
      ],
      codecs: __gh_codecs,
    }),
    stateLen: __gh_client_contract.defineContractMethod<
      ArenaServer.v0_1.StateLenArgs,
      ArenaServer.v0_1.StateLenResult,
      ArenaServer.v0_1.DeclaredException
    >({
      interfaceName: "ArenaServer",
      interfaceVersion: "0.1.0",
      abiName: "state_len",
      publicName: "stateLen",
      entrypoint: __gh_entrypoints.ArenaServer.v0_1.stateLen,
      effect: spec.interfaces[0]!.methods[4]!.effect,
      args: [
      ],
      returnType: spec.interfaces[0]!.methods[4]!.return_type,
      throws: [
        {
          selector: 0x49495a0411040b2dn,
          name: "ArenaError",
          type: spec.interfaces[0]!.throws[0]!.ty,
        },
      ],
      codecs: __gh_codecs,
    }),
    stateSlice: __gh_client_contract.defineContractMethod<
      ArenaServer.v0_1.StateSliceArgs,
      ArenaServer.v0_1.StateSliceResult,
      ArenaServer.v0_1.DeclaredException
    >({
      interfaceName: "ArenaServer",
      interfaceVersion: "0.1.0",
      abiName: "state_slice",
      publicName: "stateSlice",
      entrypoint: __gh_entrypoints.ArenaServer.v0_1.stateSlice,
      effect: spec.interfaces[0]!.methods[5]!.effect,
      args: [
        {
          abiName: "offset",
          publicName: "offset",
          type: spec.interfaces[0]!.methods[5]!.args[0]!.ty,
        },
      ],
      returnType: spec.interfaces[0]!.methods[5]!.return_type,
      throws: [
        {
          selector: 0x49495a0411040b2dn,
          name: "ArenaError",
          type: spec.interfaces[0]!.throws[0]!.ty,
        },
      ],
      codecs: __gh_codecs,
    }),
  },
} as const;

export const arena = {
  v0_1: {
    implementation: {
      ArenaError: {
        "EmptyBatch": 1n,
        "BatchTooLong": 2n,
      },
      ArenaOpened: __gh_client_contract.defineContractEvent<
        "ArenaOpened",
        arena.v0_1.implementation.ArenaOpened["topics"],
        arena.v0_1.implementation.ArenaOpened["fields"],
        arena.v0_1.implementation.ArenaOpenedFilter
      >({
        name: "ArenaOpened",
        signature: spec.events[0]!.signature,
        topics: [
        ],
        fields: [
          { name: "genesis_ns", type: spec.events[0]!.fields[0]!.ty },
          { name: "seed", type: spec.events[0]!.fields[1]!.ty },
        ],
        codecs: __gh_codecs,
      }),
      Frag: __gh_client_contract.defineContractEvent<
        "Frag",
        arena.v0_1.implementation.Frag["topics"],
        arena.v0_1.implementation.Frag["fields"],
        arena.v0_1.implementation.FragFilter
      >({
        name: "Frag",
        signature: spec.events[1]!.signature,
        topics: [
          {
            name: "tic",
            type: spec.events[1]!.topics[0]!.ty,
            representation: "u32",
          },
        ],
        fields: [
          { name: "killer", type: spec.events[1]!.fields[0]!.ty },
          { name: "victim", type: spec.events[1]!.fields[1]!.ty },
          { name: "killer_slot", type: spec.events[1]!.fields[2]!.ty },
          { name: "victim_slot", type: spec.events[1]!.fields[3]!.ty },
        ],
        codecs: __gh_codecs,
      }),
      KillCam: __gh_client_contract.defineContractEvent<
        "KillCam",
        arena.v0_1.implementation.KillCam["topics"],
        arena.v0_1.implementation.KillCam["fields"],
        arena.v0_1.implementation.KillCamFilter
      >({
        name: "KillCam",
        signature: spec.events[2]!.signature,
        topics: [
          {
            name: "tic",
            type: spec.events[2]!.topics[0]!.ty,
            representation: "u32",
          },
        ],
        fields: [
          { name: "killer_slot", type: spec.events[2]!.fields[0]!.ty },
          { name: "victim_slot", type: spec.events[2]!.fields[1]!.ty },
          { name: "frame", type: spec.events[2]!.fields[2]!.ty },
        ],
        codecs: __gh_codecs,
      }),
    },
  },
} as const;

export type ArenaServerEvent = arena.v0_1.implementation.ArenaOpened | arena.v0_1.implementation.Frag | arena.v0_1.implementation.KillCam;

export function decodeEvent(__gh_event: { topics: globalThis.Uint8Array[]; data: globalThis.Uint8Array }): ArenaServerEvent | null {
  return arena.v0_1.implementation.ArenaOpened.decode(__gh_event) ?? arena.v0_1.implementation.Frag.decode(__gh_event) ?? arena.v0_1.implementation.KillCam.decode(__gh_event);
}

// The types the namespaces above refer to, under names no declaration can capture.
export namespace __gh_codegen {
  export type arena$v0_1$implementation$ArenaError = arena.v0_1.implementation.ArenaError;
  export type arena$v0_1$implementation$CrankReport = arena.v0_1.implementation.CrankReport;
}
