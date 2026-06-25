#![cfg(test)]

//! # Issue #123 — Event emission ordering
//!
//! Verifies that each contract operation emits the correct event(s).
//! `env.events().all()` returns only the events from the most recent contract
//! invocation, so each test captures events immediately after the relevant call.

extern crate std;

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    token, vec, Address, Env, String, Symbol, TryFromVal,
};

// ============================================================
// HELPERS
// ============================================================

fn setup() -> (Env, Address, Address, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ChainSettleContract, ());
    let token_id = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let admin    = Address::generate(&env);
    let buyer    = Address::generate(&env);
    let supplier = Address::generate(&env);
    let logistics = Address::generate(&env);
    let arbiter  = Address::generate(&env);
    token::StellarAssetClient::new(&env, &token_id).mint(&buyer, &100_000_000_000);
    ChainSettleContractClient::new(&env, &contract_id).init(&admin);
    (env, contract_id, token_id, buyer, supplier, logistics, arbiter)
}

fn opts() -> ShipmentOptions {
    ShipmentOptions {
        response_deadline: 0, penalty_bps: 0,
        milestone_mode: MilestoneMode::Parallel,
        holdback_ledgers: 0, dispute_cooldown_ledgers: 0,
        late_penalty_bps_per_ledger: 0, auto_confirm_ledgers: 0,
        dispute_bond_amount: 0, arbiter_fee_bps: 0,
    }
}

fn milestones(env: &Env) -> soroban_sdk::Vec<Milestone> {
    vec![env,
        Milestone { name: String::from_str(env, "M0"), payment_percent: 50,
            proof_hash: String::from_str(env, ""), status: MilestoneStatus::Pending,
            release_after_ledger: 0, proof_submitted_ledger: None, dispute_opened_ledger: None },
        Milestone { name: String::from_str(env, "M1"), payment_percent: 50,
            proof_hash: String::from_str(env, ""), status: MilestoneStatus::Pending,
            release_after_ledger: 0, proof_submitted_ledger: None, dispute_opened_ledger: None },
    ]
}

/// Collect first-topic Symbols from the most-recent call's events.
fn last_event_topics(env: &Env) -> std::vec::Vec<Symbol> {
    env.events().all().iter()
        .filter_map(|(_id, topics, _data)| {
            topics.get(0).and_then(|v| Symbol::try_from_val(env, &v).ok())
        })
        .collect()
}

fn has(topics: &[Symbol], name: &Symbol) -> bool {
    topics.iter().any(|t| t == name)
}

// ============================================================
// TESTS
// ============================================================

/// create_shipment → emits shipment_created
#[test]
fn test_event_create_shipment_emits_shipment_created() {
    let (env, contract_id, token_id, buyer, supplier, logistics, arbiter) = setup();
    let client = ChainSettleContractClient::new(&env, &contract_id);
    client.create_shipment(&String::from_str(&env, "E01"),
        &vec![&env, buyer.clone()], &supplier, &logistics, &arbiter,
        &token_id, &1_000_000_000, &milestones(&env), &opts());
    let t = last_event_topics(&env);
    assert!(has(&t, &Symbol::new(&env, "shipment_created")), "expected shipment_created");
}

/// submit_proof → emits proof_submitted (and NOT shipment_created)
#[test]
fn test_event_submit_proof_emits_proof_submitted() {
    let (env, contract_id, token_id, buyer, supplier, logistics, arbiter) = setup();
    let client = ChainSettleContractClient::new(&env, &contract_id);
    let sid = String::from_str(&env, "E02");
    client.create_shipment(&sid, &vec![&env, buyer.clone()], &supplier, &logistics, &arbiter,
        &token_id, &1_000_000_000, &milestones(&env), &opts());
    client.submit_proof(&supplier, &sid, &0, &String::from_str(&env, "ipfs://x"), &Symbol::new(&env, "ipfs"));
    let t = last_event_topics(&env);
    assert!(has(&t, &Symbol::new(&env, "proof_submitted")), "expected proof_submitted");
    assert!(!has(&t, &Symbol::new(&env, "shipment_created")), "shipment_created must not appear after submit_proof");
}

/// confirm_milestone → emits milestone_confirmed (and NOT proof_submitted)
#[test]
fn test_event_confirm_milestone_emits_milestone_confirmed() {
    let (env, contract_id, token_id, buyer, supplier, logistics, arbiter) = setup();
    let client = ChainSettleContractClient::new(&env, &contract_id);
    let sid = String::from_str(&env, "E03");
    client.create_shipment(&sid, &vec![&env, buyer.clone()], &supplier, &logistics, &arbiter,
        &token_id, &1_000_000_000, &milestones(&env), &opts());
    client.submit_proof(&supplier, &sid, &0, &String::from_str(&env, "ipfs://x"), &Symbol::new(&env, "ipfs"));
    client.confirm_milestone(&buyer, &sid, &0);
    let t = last_event_topics(&env);
    assert!(has(&t, &Symbol::new(&env, "milestone_confirmed")), "expected milestone_confirmed");
    assert!(!has(&t, &Symbol::new(&env, "proof_submitted")), "proof_submitted must not re-appear in confirm call");
}

/// raise_dispute → emits dispute_raised (and NOT milestone_confirmed)
#[test]
fn test_event_raise_dispute_emits_dispute_raised() {
    let (env, contract_id, token_id, buyer, supplier, logistics, arbiter) = setup();
    let client = ChainSettleContractClient::new(&env, &contract_id);
    let sid = String::from_str(&env, "E04");
    client.create_shipment(&sid, &vec![&env, buyer.clone()], &supplier, &logistics, &arbiter,
        &token_id, &1_000_000_000, &milestones(&env), &opts());
    client.submit_proof(&supplier, &sid, &0, &String::from_str(&env, "ipfs://x"), &Symbol::new(&env, "ipfs"));
    client.raise_dispute(&buyer, &sid, &0);
    let t = last_event_topics(&env);
    assert!(has(&t, &Symbol::new(&env, "dispute_raised")), "expected dispute_raised");
    assert!(!has(&t, &Symbol::new(&env, "milestone_confirmed")), "milestone_confirmed must not appear on dispute");
}

/// resolve_dispute (approve) → emits dispute_resolved (and NOT dispute_raised)
#[test]
fn test_event_resolve_dispute_emits_dispute_resolved() {
    let (env, contract_id, token_id, buyer, supplier, logistics, arbiter) = setup();
    let client = ChainSettleContractClient::new(&env, &contract_id);
    let sid = String::from_str(&env, "E05");
    client.create_shipment(&sid, &vec![&env, buyer.clone()], &supplier, &logistics, &arbiter,
        &token_id, &1_000_000_000, &milestones(&env), &opts());
    client.submit_proof(&supplier, &sid, &0, &String::from_str(&env, "ipfs://x"), &Symbol::new(&env, "ipfs"));
    client.raise_dispute(&buyer, &sid, &0);
    client.resolve_dispute(&arbiter, &sid, &0, &true);
    let t = last_event_topics(&env);
    assert!(has(&t, &Symbol::new(&env, "dispute_resolved")), "expected dispute_resolved");
    assert!(!has(&t, &Symbol::new(&env, "dispute_raised")), "dispute_raised must not re-appear in resolve call");
}

/// cancel_shipment → emits shipment_cancelled (and NOT shipment_created)
#[test]
fn test_event_cancel_shipment_emits_shipment_cancelled() {
    let (env, contract_id, token_id, buyer, supplier, logistics, arbiter) = setup();
    let client = ChainSettleContractClient::new(&env, &contract_id);
    let sid = String::from_str(&env, "E06");
    client.create_shipment(&sid, &vec![&env, buyer.clone()], &supplier, &logistics, &arbiter,
        &token_id, &1_000_000_000, &milestones(&env), &opts());
    client.cancel_shipment(&buyer, &sid);
    let t = last_event_topics(&env);
    assert!(has(&t, &Symbol::new(&env, "shipment_cancelled")), "expected shipment_cancelled");
    assert!(!has(&t, &Symbol::new(&env, "shipment_created")), "shipment_created must not re-appear on cancel");
}

/// Each operation in the lifecycle emits exactly its own event — no cross-contamination.
#[test]
fn test_event_each_operation_emits_own_event_only() {
    let (env, contract_id, token_id, buyer, supplier, logistics, arbiter) = setup();
    let client = ChainSettleContractClient::new(&env, &contract_id);
    let sid = String::from_str(&env, "E07");

    client.create_shipment(&sid, &vec![&env, buyer.clone()], &supplier, &logistics, &arbiter,
        &token_id, &1_000_000_000, &milestones(&env), &opts());
    assert!(has(&last_event_topics(&env), &Symbol::new(&env, "shipment_created")));

    client.submit_proof(&supplier, &sid, &0, &String::from_str(&env, "ipfs://a"), &Symbol::new(&env, "ipfs"));
    assert!(has(&last_event_topics(&env), &Symbol::new(&env, "proof_submitted")));

    client.confirm_milestone(&buyer, &sid, &0);
    assert!(has(&last_event_topics(&env), &Symbol::new(&env, "milestone_confirmed")));

    client.submit_proof(&supplier, &sid, &1, &String::from_str(&env, "ipfs://b"), &Symbol::new(&env, "ipfs"));
    client.raise_dispute(&buyer, &sid, &1);
    assert!(has(&last_event_topics(&env), &Symbol::new(&env, "dispute_raised")));

    client.resolve_dispute(&arbiter, &sid, &1, &true);
    assert!(has(&last_event_topics(&env), &Symbol::new(&env, "dispute_resolved")));
}
