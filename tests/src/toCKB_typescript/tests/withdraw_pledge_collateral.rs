use super::{Error, Script, ToCKBCellData};
use crate::toCKB_typescript::utils::config::*;
use crate::*;
use ckb_testtool::{builtin::ALWAYS_SUCCESS, context::Context};
use ckb_tool::ckb_types::{
    bytes::Bytes,
    core::{TransactionBuilder, TransactionView},
    packed::*,
    prelude::*,
};
use ckb_tool::{ckb_error::assert_error_eq, ckb_script::ScriptError};
use molecule::prelude::*;
use toCKB_typescript::utils::types::generated::*;

const MAX_CYCLES: u64 = 10_000_000;

#[test]
fn test_correct_tx() {
    let (context, tx) = build_test_context(SINCE_WITHDRAW_PLEDGE_COLLATERAL, 100, 100);
    let cycles = context
        .verify_tx(&tx, MAX_CYCLES)
        .expect("pass verification");
    println!("consume cycles: {}", cycles);
}

#[test]
fn test_correct_tx_when_output_capacity_bigger_than_pledge_collateral() {
    let (context, tx) = build_test_context(SINCE_WITHDRAW_PLEDGE_COLLATERAL, 100, 101);
    let cycles = context
        .verify_tx(&tx, MAX_CYCLES)
        .expect("pass verification");
    println!("consume cycles: {}", cycles);
}

#[test]
fn test_wrong_tx_since_mismatch() {
    let (context, tx) = build_test_context(SINCE_WITHDRAW_PLEDGE_COLLATERAL + 1, 100, 100);
    let err = context.verify_tx(&tx, MAX_CYCLES).unwrap_err();
    assert_error_eq!(
        err,
        ScriptError::ValidationFailure(Error::InputSinceInvalid as i8)
    );
}

#[test]
fn test_wrong_tx_capacity_mismatch() {
    let (context, tx) = build_test_context(SINCE_WITHDRAW_PLEDGE_COLLATERAL, 100, 99);
    let err = context.verify_tx(&tx, MAX_CYCLES).unwrap_err();
    assert_error_eq!(
        err,
        ScriptError::ValidationFailure(Error::CapacityInvalid as i8)
    );
}

fn build_test_context(
    since: u64,
    input_capacity: u64,
    output_capacity: u64,
) -> (Context, TransactionView) {
    // deploy contract
    let mut context = Context::default();
    let toCKB_typescript_bin: Bytes = Loader::default().load_binary("toCKB-typescript");
    let toCKB_typescript_out_point = context.deploy_cell(toCKB_typescript_bin);
    let always_success_out_point = context.deploy_cell(ALWAYS_SUCCESS.clone());
    // prepare scripts
    let toCKB_typescript = context
        .build_script(&toCKB_typescript_out_point, [2; 1].to_vec().into())
        .expect("script");
    let toCKB_typescript_dep = CellDep::new_builder()
        .out_point(toCKB_typescript_out_point)
        .build();
    let always_success_lockscript = context
        .build_script(&always_success_out_point, Default::default())
        .expect("script");
    let always_success_lockscript_dep = CellDep::new_builder()
        .out_point(always_success_out_point)
        .build();

    let signer_lockscript =
        basic::Script::from_slice(always_success_lockscript.as_slice()).unwrap();

    let input_toCKB_data = ToCKBCellData::new_builder()
        .status(Byte::new(2u8))
        .lot_size(Byte::new(1u8))
        .user_lockscript(Script::new_builder().build())
        .x_lock_address(
            basic::Bytes::new_builder()
                .set([Byte::new(1u8); 20].to_vec())
                .build(),
        )
        .signer_lockscript(signer_lockscript)
        .build();

    let input_ckb_cell_out_point = context.create_cell(
        CellOutput::new_builder()
            .capacity((input_capacity * CKB_UNITS).pack())
            .lock(always_success_lockscript.clone())
            .type_(Some(toCKB_typescript.clone()).pack())
            .build(),
        input_toCKB_data.as_bytes(),
    );
    let input_ckb_cell = CellInput::new_builder()
        .previous_output(input_ckb_cell_out_point)
        .since(since.pack())
        .build();

    let inputs = vec![input_ckb_cell];

    let output_cell = CellOutput::new_builder()
        .capacity((output_capacity * CKB_UNITS).pack())
        .lock(always_success_lockscript.clone())
        .build();
    let outputs = vec![output_cell];
    let outputs_data = vec![Bytes::new(); 1];

    let tx = TransactionBuilder::default()
        .inputs(inputs)
        .outputs(outputs)
        .outputs_data(outputs_data.pack())
        .cell_dep(toCKB_typescript_dep)
        .cell_dep(always_success_lockscript_dep)
        .build();
    let tx = context.complete_tx(tx);

    (context, tx)
}
