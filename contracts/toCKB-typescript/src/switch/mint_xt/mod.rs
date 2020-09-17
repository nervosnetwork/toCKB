use crate::switch::ToCKBCellDataTuple;
use crate::utils::{
    config::{
        PLEDGE, SIGNER_FEE_RATE, SUDT_CODE_HASH, TX_PROOF_DIFFICULTY_FACTOR, XT_CELL_CAPACITY,
    },
    tools::{get_xchain_kind, is_XT_typescript, verify_btc_witness, XChainKind},
    types::{
        btc_difficulty::BTCDifficultyReader,
        mint_xt_witness::{BTCSPVProofReader, ETHSPVProofReader, MintXTWitnessReader},
        Error, ToCKBCellDataView, XExtraView
    },
};
use alloc::string::String;
use alloc::vec;
use bech32::ToBase32;
use bitcoin_spv::{
    btcspv,
    types::{HeaderArray, MerkleArray, PayloadType, Vin, Vout},
    validatespv,
};
use ckb_std::{
    ckb_constants::Source,
    debug,
    high_level::{
        load_cell_capacity, load_cell_data, load_cell_lock, load_cell_lock_hash, load_cell_type,
        load_witness_args, QueryIter,
    },
};
use core::result::Result;
use eth_spv_lib::{eth_types::*, ethspv};
use molecule::prelude::{Entity, Reader};
use primitive_types::U256;
use rlp;
use crate::utils::types::EthExtraView;

fn verify_data(
    input_data: &ToCKBCellDataView,
    output_data: &ToCKBCellDataView,
    x_extra: &XExtraView,
) -> Result<(), Error> {
    if input_data.signer_lockscript != output_data.signer_lockscript
        || input_data.user_lockscript != output_data.user_lockscript
        || input_data.get_raw_lot_size() != output_data.get_raw_lot_size()
        || input_data.x_lock_address != output_data.x_lock_address
        || &output_data.x_extra != x_extra
    {
        return Err(Error::InvalidDataChange);
    }
    Ok(())
}

/// ensure transfer happen on XChain by verifying the spv proof
fn verify_witness(data: &ToCKBCellDataView) -> Result<XExtraView, Error> {
    let witness_args = load_witness_args(0, Source::GroupInput)?.input_type();
    debug!("witness_args: {:?}", &witness_args);
    if witness_args.is_none() {
        return Err(Error::InvalidWitness);
    }
    let witness_args = witness_args.to_opt().unwrap().raw_data();
    debug!("witness_args parsed: {:?}", &witness_args);
    if MintXTWitnessReader::verify(&witness_args, false).is_err() {
        return Err(Error::InvalidWitness);
    }
    let witness = MintXTWitnessReader::new_unchecked(&witness_args);
    debug!("witness: {:?}", witness);
    let proof = witness.spv_proof().raw_data();
    let cell_dep_index_list = witness.cell_dep_index_list().raw_data();
    match data.get_xchain_kind() {
        XChainKind::Btc => {
            let btc_extra = verify_btc_witness(
                data,
                proof,
                cell_dep_index_list,
                data.x_lock_address.as_ref(),
                data.get_btc_lot_size()?.get_sudt_amount(),
                false,
            )?;
            Ok(XExtraView::Btc(btc_extra))
        }
        XChainKind::Eth => {
            let eth_extra = verify_eth_witness(data, proof, cell_dep_index_list)?;
        Ok(XExtraView::Eth(eth_extra))},
    }
}

fn verify_eth_witness(
    data: &ToCKBCellDataView,
    proof: &[u8],
    cell_dep_index_list: &[u8],
) -> Result<EthExtraView, Error> {
    debug!(
        "proof: {:?}, cell_dep_index_list: {:?}",
        proof, cell_dep_index_list
    );
    if ETHSPVProofReader::verify(proof, false).is_err() {
        return Err(Error::InvalidWitness);
    }
    let proof_reader = ETHSPVProofReader::new_unchecked(proof);
    debug!("proof_reader: {:?}", proof_reader);
    //TODO: verify header with client
    // hash = calc(header_data)
    // hash.compare(client.getHashByNumber(header.number))
    // verify eth spv
    let mut log_index = [0u8; 8];
    log_index.copy_from_slice(proof_reader.log_index().raw_data());
    debug!("log_index is {:?}", &log_index);
    let log_entry_data = proof_reader.log_entry_data().raw_data().to_vec();
    debug!("log_entry_data is {:?}", &log_entry_data);
    let receipt_data = proof_reader.receipt_data().raw_data().to_vec();
    debug!("receipt_data is {:?}", &receipt_data);
    let mut receipt_index = [0u8; 8];
    receipt_index.copy_from_slice(proof_reader.receipt_index().raw_data());
    debug!("receipt_index is {:?}", &receipt_index);
    let mut receipts_root = [0u8; 32];
    receipts_root.copy_from_slice(proof_reader.receipts_root().raw_data());
    debug!("receipts_root is {:?}", &receipts_root);
    let mut proof = vec![];
    for i in 0..proof_reader.proof().len() {
        proof.push(proof_reader.proof().get_unchecked(i).raw_data().to_vec());
    }
    debug!("proof is {:?}", &proof);
    let log_entry: LogEntry = rlp::decode(log_entry_data.as_slice()).unwrap();
    debug!("log_entry is {:?}", &log_entry);
    let receipt: Receipt = rlp::decode(receipt_data.as_slice()).unwrap();
    debug!("receipt_data is {:?}", &receipt);
    let locker_address = (log_entry.address.clone().0).0;
    debug!(
        "addr: {:?}, x_lock_address: {}",
        hex::encode(locker_address.to_vec()),
        String::from_utf8(data.x_lock_address.as_ref().to_vec()).unwrap()
    );
    // assert_eq!(hex::encode(locker_address.to_vec()),
    //            String::from_utf8(data.x_lock_address.as_ref().to_vec()).unwrap(),
    //            "the x lock address should be equal with the address from data.");
    if hex::encode(locker_address.to_vec()) != String::from_utf8(data.x_lock_address.as_ref().to_vec()).unwrap() {
        return Err(Error::WrongFundingAddr)
    }
    assert_eq!(ethspv::verify_log_entry(
        u64::from_le_bytes(log_index),
        log_entry_data,
        u64::from_le_bytes(receipt_index),
        receipt_data,
        H256(receipts_root.into()),
        proof,
    ), true, "invalid eth spv proof.");
    Ok(EthExtraView {
        dummy: Default::default(),
    })
}

fn verify_xt_issue(data: &ToCKBCellDataView) -> Result<(), Error> {
    let lock_hash = load_cell_lock_hash(0, Source::GroupInput)?;
    debug!("lockscript hash: {:?}", hex::encode(lock_hash));
    let input_xt_num = QueryIter::new(load_cell_type, Source::Input)
        .filter(|type_opt| type_opt.is_some())
        .map(|type_opt| type_opt.unwrap())
        .filter(|script| is_XT_typescript(script, lock_hash.as_ref()))
        .count();
    if input_xt_num != 0 {
        return Err(Error::InvalidXTInInputOrOutput);
    }
    let output_xt_num = QueryIter::new(load_cell_type, Source::Output)
        .filter(|type_opt| type_opt.is_some())
        .map(|type_opt| type_opt.unwrap())
        .filter(|script| is_XT_typescript(script, lock_hash.as_ref()))
        .count();
    debug!("output_xt_num: {}", output_xt_num);
    if output_xt_num != 2 {
        return Err(Error::InvalidXTInInputOrOutput);
    }
    let xt_amount = match get_xchain_kind()? {
        XChainKind::Btc => data.get_btc_lot_size()?.get_sudt_amount(),
        XChainKind::Eth => data.get_eth_lot_size()?.get_sudt_amount(),
    };
    // data.get_btc_lot_size()?.get_sudt_amount();
    debug!("xt_amount: {}", xt_amount);
    // fixed order of output cells is required
    // user-sudt-cell should be outputs[1]
    // signer-sudt-cell should be outputs[2]
    let expect = [
        (
            1,
            data.user_lockscript.as_ref(),
            xt_amount - xt_amount * SIGNER_FEE_RATE.0 / SIGNER_FEE_RATE.1,
        ),
        (
            2,
            data.signer_lockscript.as_ref(),
            xt_amount * SIGNER_FEE_RATE.0 / SIGNER_FEE_RATE.1,
        ),
    ];
    debug!("expect: {:?}", expect);

    for (i, lockscript, amount) in expect.iter() {
        let script = load_cell_type(*i, Source::Output)?;
        if script.is_none() {
            return Err(Error::InvalidMintOutput);
        }
        let script = script.unwrap();
        if !(script.code_hash().raw_data().as_ref() == SUDT_CODE_HASH.as_ref()
            && script.args().raw_data().as_ref() == lock_hash.as_ref()
            && script.hash_type() == 0u8.into())
        {
            return Err(Error::InvalidMintOutput);
        }
        let cell_data = load_cell_data(*i, Source::Output)?;
        let mut amount_vec = [0u8; 16];
        amount_vec.copy_from_slice(&cell_data);
        let token_amount = u128::from_le_bytes(amount_vec);
        debug!("token_amount: {}, amout: {}", token_amount, amount);
        if token_amount != *amount {
            return Err(Error::InvalidMintOutput);
        }
        let lock = load_cell_lock(*i, Source::Output)?;
        debug!(
            "lock: {:?}, expect lock: {:?}",
            hex::encode(lock.as_slice()),
            hex::encode(lockscript.as_ref())
        );
        if lock.as_slice() != lockscript.as_ref() {
            return Err(Error::InvalidMintOutput);
        }
    }
    Ok(())
}

pub fn verify_capacity() -> Result<(), Error> {
    let toCKB_output_cap = load_cell_capacity(0, Source::GroupOutput)?;
    let toCKB_input_cap = load_cell_capacity(0, Source::GroupInput)?;
    if toCKB_input_cap - toCKB_output_cap != PLEDGE + XT_CELL_CAPACITY {
        return Err(Error::CapacityInvalid);
    }
    let user_xt_cell_cap = load_cell_capacity(1, Source::Output)?;
    if user_xt_cell_cap != PLEDGE {
        return Err(Error::CapacityInvalid);
    }
    let signer_xt_cell_cap = load_cell_capacity(2, Source::Output)?;
    if signer_xt_cell_cap != XT_CELL_CAPACITY {
        return Err(Error::CapacityInvalid);
    }
    Ok(())
}

pub fn verify(toCKB_data_tuple: &ToCKBCellDataTuple) -> Result<(), Error> {
    debug!("start mint_xt");
    let input_data = toCKB_data_tuple.0.as_ref().expect("should not happen");
    let output_data = toCKB_data_tuple.1.as_ref().expect("should not happen");
    verify_capacity()?;
    let x_extra = verify_witness(input_data)?;
    debug!("verify witness finish");
    verify_data(input_data, output_data, &x_extra)?;
    debug!("verify data finish");
    verify_xt_issue(input_data)?;
    debug!("verify xt issue finish");
    Ok(())
}
