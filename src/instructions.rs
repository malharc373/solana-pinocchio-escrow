use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, Address, ProgramResult,
};

use crate::{
    accounts::{check_programs, check_signer, close_escrow, create_escrow, find_pda, load_escrow},
    state::Escrow,
    token::{ata_balance, close_vault, create_ata, mint_decimals, TransferChecked},
    EscrowError, ESCROW_SEED,
};

pub fn make(
    program: &Address,
    accounts: &mut [AccountView],
    seed: u64,
    deposit: u64,
    receive: u64,
) -> ProgramResult {
    let [maker, mint_a, mint_b, maker_a, escrow, vault, associated, token, system, ..] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    check_signer(maker)?;
    check_programs(associated, token, system)?;
    if deposit == 0 || receive == 0 {
        return Err(EscrowError::ZeroAmount.into());
    }
    if mint_a.address() == mint_b.address() {
        return Err(EscrowError::SameMint.into());
    }

    let token_program = token.address();
    let decimals = mint_decimals(mint_a, token_program)?;
    mint_decimals(mint_b, token_program)?;
    ata_balance(maker_a, maker.address(), mint_a.address(), token_program)?;

    let seed_bytes = seed.to_le_bytes();
    let (expected, bump) = find_pda(
        &[ESCROW_SEED, maker.address().as_ref(), &seed_bytes],
        program,
    )?;
    if escrow.address() != &expected {
        return Err(ProgramError::InvalidSeeds);
    }
    let bump_bytes = [bump];
    let seeds = escrow_seeds(maker.address(), &seed_bytes, &bump_bytes);
    create_escrow(maker, escrow, program, &[Signer::from(&seeds)])?;
    create_ata(maker, vault, escrow, mint_a, system, token)?;

    Escrow {
        seed,
        maker: *maker.address(),
        mint_a: *mint_a.address(),
        mint_b: *mint_b.address(),
        receive,
        bump,
    }
    .pack(&mut escrow.try_borrow_mut()?);
    TransferChecked {
        from: maker_a,
        mint: mint_a,
        to: vault,
        authority: maker,
        amount: deposit,
        decimals,
    }
    .invoke_signed(token_program, &[])
}

pub fn take(program: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [taker, maker, mint_a, mint_b, taker_a, taker_b, maker_b, escrow, vault, associated, token, system, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    check_signer(taker)?;
    check_programs(associated, token, system)?;
    if !maker.owned_by(&pinocchio_system::ID) {
        return Err(ProgramError::IncorrectProgramId);
    }
    let state = load_escrow(escrow, program, maker, mint_a)?;
    if &state.mint_b != mint_b.address() {
        return Err(EscrowError::InvalidMintB.into());
    }

    let token_program = token.address();
    let decimals_a = mint_decimals(mint_a, token_program)?;
    let decimals_b = mint_decimals(mint_b, token_program)?;
    let amount = ata_balance(vault, escrow.address(), mint_a.address(), token_program)?;
    ata_balance(taker_b, taker.address(), mint_b.address(), token_program)?;
    ensure_ata(taker, taker_a, taker, mint_a, system, token)?;
    ensure_ata(taker, maker_b, maker, mint_b, system, token)?;

    TransferChecked {
        from: taker_b,
        mint: mint_b,
        to: maker_b,
        authority: taker,
        amount: state.receive,
        decimals: decimals_b,
    }
    .invoke_signed(token_program, &[])?;
    settle(
        escrow,
        vault,
        maker,
        taker_a,
        mint_a,
        token_program,
        &state,
        amount,
        decimals_a,
    )
}

pub fn refund(program: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [maker, mint_a, maker_a, escrow, vault, associated, token, system, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    check_signer(maker)?;
    check_programs(associated, token, system)?;
    let state = load_escrow(escrow, program, maker, mint_a)?;

    let token_program = token.address();
    let decimals = mint_decimals(mint_a, token_program)?;
    let amount = ata_balance(vault, escrow.address(), mint_a.address(), token_program)?;
    ensure_ata(maker, maker_a, maker, mint_a, system, token)?;
    settle(
        escrow,
        vault,
        maker,
        maker_a,
        mint_a,
        token_program,
        &state,
        amount,
        decimals,
    )
}

fn ensure_ata(
    payer: &AccountView,
    account: &AccountView,
    owner: &AccountView,
    mint: &AccountView,
    system: &AccountView,
    token: &AccountView,
) -> ProgramResult {
    if account.owned_by(&pinocchio_system::ID) {
        // The ATA program enforces the canonical address and initializes its fields.
        create_ata(payer, account, owner, mint, system, token)
    } else {
        ata_balance(account, owner.address(), mint.address(), token.address()).map(|_| ())
    }
}

fn escrow_seeds<'a>(maker: &'a Address, seed: &'a [u8; 8], bump: &'a [u8; 1]) -> [Seed<'a>; 4] {
    [
        Seed::from(ESCROW_SEED),
        Seed::from(maker.as_ref()),
        Seed::from(seed),
        Seed::from(bump),
    ]
}

#[allow(clippy::too_many_arguments)]
fn settle(
    escrow: &mut AccountView,
    vault: &AccountView,
    maker: &mut AccountView,
    destination: &AccountView,
    mint: &AccountView,
    token_program: &Address,
    state: &Escrow,
    amount: u64,
    decimals: u8,
) -> ProgramResult {
    let seed_bytes = state.seed.to_le_bytes();
    let bump_bytes = [state.bump];
    let seeds = escrow_seeds(maker.address(), &seed_bytes, &bump_bytes);
    let signers = [Signer::from(&seeds)];
    TransferChecked {
        from: vault,
        mint,
        to: destination,
        authority: escrow,
        amount,
        decimals,
    }
    .invoke_signed(token_program, &signers)?;
    close_vault(vault, maker, escrow, token_program, &signers)?;
    close_escrow(escrow, maker)
}
