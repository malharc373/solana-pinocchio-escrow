//! CPI encodings and shared base fields for SPL Token and Token-2022.
use pinocchio::{
    cpi::{invoke, invoke_signed, Signer},
    error::ProgramError,
    instruction::{InstructionAccount, InstructionView},
    AccountView, Address, ProgramResult,
};

use crate::accounts::find_pda;

pub const TOKEN_PROGRAM: Address =
    Address::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const TOKEN_2022_PROGRAM: Address =
    Address::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
pub const ASSOCIATED_TOKEN_PROGRAM: Address =
    Address::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub fn mint_decimals(mint: &AccountView, program: &Address) -> Result<u8, ProgramError> {
    if !mint.owned_by(program) {
        return Err(ProgramError::IncorrectProgramId);
    }
    let data = mint.try_borrow()?;
    // Token-2022 extensions follow the shared SPL Mint base layout.
    if data.len() < 82 || data[45] != 1 {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(data[44])
}

pub fn ata_balance(
    account: &AccountView,
    owner: &Address,
    mint: &Address,
    program: &Address,
) -> Result<u64, ProgramError> {
    if !account.owned_by(program) {
        return Err(ProgramError::IncorrectProgramId);
    }
    let expected = find_pda(
        &[owner.as_ref(), program.as_ref(), mint.as_ref()],
        &ASSOCIATED_TOKEN_PROGRAM,
    )?
    .0;
    let data = account.try_borrow()?;
    if account.address() != &expected
        || data.len() < 165
        || &data[..32] != mint.as_ref()
        || &data[32..64] != owner.as_ref()
        || !matches!(data[108], 1 | 2)
    {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(u64::from_le_bytes(data[64..72].try_into().unwrap()))
}

pub fn create_ata(
    payer: &AccountView,
    account: &AccountView,
    owner: &AccountView,
    mint: &AccountView,
    system: &AccountView,
    token: &AccountView,
) -> ProgramResult {
    let accounts = [
        InstructionAccount::writable_signer(payer.address()),
        InstructionAccount::writable(account.address()),
        InstructionAccount::readonly(owner.address()),
        InstructionAccount::readonly(mint.address()),
        InstructionAccount::readonly(system.address()),
        InstructionAccount::readonly(token.address()),
    ];
    invoke(
        &InstructionView {
            program_id: &ASSOCIATED_TOKEN_PROGRAM,
            accounts: &accounts,
            data: &[0], // Create
        },
        &[payer, account, owner, mint, system, token],
    )
}

pub struct TransferChecked<'a> {
    pub from: &'a AccountView,
    pub mint: &'a AccountView,
    pub to: &'a AccountView,
    pub authority: &'a AccountView,
    pub amount: u64,
    pub decimals: u8,
}

impl TransferChecked<'_> {
    pub fn invoke_signed(&self, program: &Address, signers: &[Signer]) -> ProgramResult {
        let mut data = [12, 0, 0, 0, 0, 0, 0, 0, 0, self.decimals];
        data[1..9].copy_from_slice(&self.amount.to_le_bytes());
        let accounts = [
            InstructionAccount::writable(self.from.address()),
            InstructionAccount::readonly(self.mint.address()),
            InstructionAccount::writable(self.to.address()),
            InstructionAccount::readonly_signer(self.authority.address()),
        ];
        invoke_signed(
            &InstructionView {
                program_id: program,
                accounts: &accounts,
                data: &data,
            },
            &[self.from, self.mint, self.to, self.authority],
            signers,
        )
    }
}

pub fn close_vault(
    vault: &AccountView,
    maker: &AccountView,
    authority: &AccountView,
    program: &Address,
    signers: &[Signer],
) -> ProgramResult {
    let accounts = [
        InstructionAccount::writable(vault.address()),
        InstructionAccount::writable(maker.address()),
        InstructionAccount::readonly_signer(authority.address()),
    ];
    invoke_signed(
        &InstructionView {
            program_id: program,
            accounts: &accounts,
            data: &[9],
        },
        &[vault, maker, authority],
        signers,
    )
}
