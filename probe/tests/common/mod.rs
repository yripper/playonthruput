#![allow(dead_code)]

use probe::ContractClient;
use thruput_sdk::*;

static NONCE: SHA256Hash = thruput_sdk::sha256_static!("PROBE_NONCE");

// Both test entry points call this function with their registered contract.
pub fn initializes(env: &TestEnvironmentHandle, contract_hash: &SHA256Hash) {
    let result = env.transact(|tx| {
        tx.call(&ContractAddress::named("test_deploy")).run(|| {
            let address = deploy_contract(&NONCE, contract_hash);
            ContractClient::new(&address).initialize();
        })
    });
    result.assert_success();
    result.assert_no_events();
}

/// Deploys the probe and runs `f` against it inside one transaction.
pub fn with_probe(
    env: &TestEnvironmentHandle,
    contract_hash: &SHA256Hash,
    label: &str,
    f: impl FnOnce(&ContractClient) + std::panic::UnwindSafe + 'static,
) {
    let hash = contract_hash.clone();
    env.transact(|tx| {
        tx.call(&ContractAddress::named(label)).run(move || {
            let address = deploy_contract(&NONCE, &hash);
            f(&ContractClient::new(&address));
        })
    })
    .assert_success();
}
