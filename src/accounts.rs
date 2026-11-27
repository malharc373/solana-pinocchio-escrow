use pinocchio::{
    cpi::Signer,
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::{Allocate, Assign, CreateAccount, Transfer};

use crate::{
    state::Escrow,
    token::{ASSOCIATED_TOKEN_PROGRAM, TOKEN_2022_PROGRAM, TOKEN_PROGRAM},
    EscrowError, ESCROW_SEED,
};

pub fn check_signer(account: &AccountView) -> ProgramResult {
    if !account.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(())
}

pub fn check_programs(
    associated: &AccountView,
    token: &AccountView,
    system: &AccountView,
) -> ProgramResult {
    if associated.address() != &ASSOCIATED_TOKEN_PROGRAM
        || !matches!(*token.address(), TOKEN_PROGRAM | TOKEN_2022_PROGRAM)
        || system.address() != &pinocchio_system::ID
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    Ok(())
}

pub fn find_pda(seeds: &[&[u8]], program: &Address) -> Result<(Address, u8), ProgramError> {
    #[cfg(target_os = "solana")]
    {
        Address::try_find_program_address(seeds, program).ok_or(ProgramError::InvalidSeeds)
    }
    #[cfg(not(target_os = "solana"))]
    {
        let _ = (seeds, program);
        Err(ProgramError::InvalidSeeds)
    }
}

pub fn create_escrow(
    payer: &AccountView,
    account: &AccountView,
    program: &Address,
    signer: &[Signer],
) -> ProgramResult {
    if !account.owned_by(&pinocchio_system::ID) || !account.is_data_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    let rent = Rent::get()?.try_minimum_balance(Escrow::LEN)?;
    let current = account.lamports();
    let space = Escrow::LEN as u64;

    if current == 0 {
        return CreateAccount {
            from: payer,
            to: account,
            lamports: rent,
            space,
            owner: program,
        }
        .invoke_signed(signer);
    }

    // A funded system account cannot be created again. Allocate it in place.
    if current < rent {
        Transfer {
            from: payer,
            to: account,
            lamports: rent - current,
        }
        .invoke()?;
    }
    Allocate { account, space }.invoke_signed(signer)?;
    Assign {
        account,
        owner: program,
    }
    .invoke_signed(signer)
}

pub fn load_escrow(
    account: &AccountView,
    program: &Address,
    maker: &AccountView,
    mint_a: &AccountView,
) -> Result<Escrow, ProgramError> {
    if !account.owned_by(program) {
        return Err(ProgramError::IncorrectProgramId);
    }
    let state = Escrow::unpack(&account.try_borrow()?)?;
    if &state.maker != maker.address() {
        return Err(EscrowError::InvalidMaker.into());
    }
    if &state.mint_a != mint_a.address() {
        return Err(EscrowError::InvalidMintA.into());
    }
    // The bump was checked at creation. Re-derive the address without
    // searching for a bump again.
    let expected = Address::derive_address(
        &[
            ESCROW_SEED,
            maker.address().as_ref(),
            &state.seed.to_le_bytes(),
        ],
        Some(state.bump),
        program,
    );
    if account.address() != &expected {
        return Err(ProgramError::InvalidSeeds);
    }
    Ok(state)
}

pub fn close_escrow(account: &mut AccountView, maker: &mut AccountView) -> ProgramResult {
    if !account.is_writable() || !maker.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    let balance = maker
        .lamports()
        .checked_add(account.lamports())
        .ok_or(ProgramError::ArithmeticOverflow)?;
    maker.set_lamports(balance);
    account.close()
}
