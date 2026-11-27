//! LiteSVM integration tests ported from solana-escrow.

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{
            instruction::{AccountMeta, Instruction},
            system_instruction, system_program,
        },
        AccountDeserialize,
    },
    anchor_spl::{
        associated_token::{
            self, get_associated_token_address_with_program_id,
            spl_associated_token_account::instruction::create_associated_token_account,
        },
        token::ID as TOKEN_PROGRAM_ID,
        token_2022::{spl_token_2022, ID as TOKEN_2022_PROGRAM_ID},
        token_interface::TokenAccount,
    },
    litesvm::LiteSVM,
    pinocchio_escrow::{state::Escrow, ESCROW_SEED, MAKE, REFUND, TAKE},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const PINOCCHIO_ID: Pubkey = Pubkey::new_from_array([77; 32]);
const ANCHOR_ID: Pubkey = Pubkey::from_str_const("8emUGfskjPAGb2DaxtkgtcpSZhwfgoa3a6f8VipzSB5M");
const PINOCCHIO_SO: &[u8] = include_bytes!(concat!(
    env!("CARGO_TARGET_TMPDIR"),
    "/../deploy/pinocchio_escrow.so"
));
const ANCHOR_SO: &[u8] = include_bytes!("../bench/anchor_escrow.so");
const ANCHOR_NO_LOG_SO: &[u8] = include_bytes!("../bench/anchor_escrow_nolog.so");

const SOL: u64 = 1_000_000_000;
const DECIMALS: u8 = 6;
const MINT_LEN: usize = 82;
const TOKEN_PROGRAMS: [Pubkey; 2] = [TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID];

struct Env {
    svm: LiteSVM,
    program: Pubkey,
    token_program: Pubkey,
    maker: Keypair,
    taker: Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
}

/// Signs and sends, returning compute units on success and logs on failure.
fn send(
    svm: &mut LiteSVM,
    ixs: &[Instruction],
    payer: &Keypair,
    signers: &[&Keypair],
) -> Result<u64, String> {
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    let res = svm
        .send_transaction(tx)
        .map(|meta| meta.compute_units_consumed)
        .map_err(|failed| format!("{:?}\n{}", failed.err, failed.meta.logs.join("\n")));
    svm.expire_blockhash();
    res
}

fn ata(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    get_associated_token_address_with_program_id(owner, mint, token_program)
}

fn balance(svm: &LiteSVM, token_account: &Pubkey) -> u64 {
    let acc = svm
        .get_account(token_account)
        .expect("token account exists");
    TokenAccount::try_deserialize(&mut acc.data.as_slice())
        .unwrap()
        .amount
}

fn is_closed(svm: &LiteSVM, address: &Pubkey) -> bool {
    svm.get_account(address)
        .is_none_or(|a| a.lamports == 0 && a.data.is_empty())
}

fn create_mint(
    svm: &mut LiteSVM,
    mint: &Keypair,
    payer: &Keypair,
    token_program: &Pubkey,
) -> Pubkey {
    let ixs = [
        system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            svm.minimum_balance_for_rent_exemption(MINT_LEN),
            MINT_LEN as u64,
            token_program,
        ),
        spl_token_2022::instruction::initialize_mint2(
            token_program,
            &mint.pubkey(),
            &payer.pubkey(),
            Some(&payer.pubkey()),
            DECIMALS,
        )
        .unwrap(),
    ];
    send(svm, &ixs, payer, &[payer, mint]).unwrap();
    mint.pubkey()
}

/// Creates `owner`'s ATA for `mint` and mints `amount` into it (`authority` = mint authority).
fn fund(
    svm: &mut LiteSVM,
    authority: &Keypair,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
    amount: u64,
) {
    let ixs = [
        create_associated_token_account(&authority.pubkey(), owner, mint, token_program),
        spl_token_2022::instruction::mint_to(
            token_program,
            mint,
            &ata(owner, mint, token_program),
            &authority.pubkey(),
            &[],
            amount,
        )
        .unwrap(),
    ];
    send(svm, &ixs, authority, &[authority]).unwrap();
}

/// Maker holds 1_000 A, taker holds 1_000 B.
fn setup(token_program: Pubkey) -> Env {
    setup_program(token_program, PINOCCHIO_ID, PINOCCHIO_SO)
}

#[allow(deprecated)]
fn setup_program(token_program: Pubkey, program: Pubkey, binary: &[u8]) -> Env {
    let mut svm = LiteSVM::new();
    // Match the current 5080-lamports-per-byte schedule expected by Pinocchio's
    // rent sysvar. LiteSVM 0.10 defaults to the legacy (3480, 2.0) schedule.
    svm.set_sysvar(&solana_rent::Rent {
        lamports_per_byte_year: 5080,
        exemption_threshold: 1.0,
        burn_percent: 50,
    });
    svm.add_program(program, binary).unwrap();

    // Fixed keys: the same PDAs and bumps, so the same compute units, every run.
    let [maker, taker, mint_a, mint_b] = [1, 2, 3, 4].map(|n| Keypair::new_from_array([n; 32]));
    svm.airdrop(&maker.pubkey(), 10 * SOL).unwrap();
    svm.airdrop(&taker.pubkey(), 10 * SOL).unwrap();

    // The maker is mint authority for both mints in these tests.
    let mint_a = create_mint(&mut svm, &mint_a, &maker, &token_program);
    let mint_b = create_mint(&mut svm, &mint_b, &maker, &token_program);
    fund(
        &mut svm,
        &maker,
        &maker.pubkey(),
        &mint_a,
        &token_program,
        1_000,
    );
    fund(
        &mut svm,
        &maker,
        &taker.pubkey(),
        &mint_b,
        &token_program,
        1_000,
    );

    Env {
        svm,
        program,
        token_program,
        maker,
        taker,
        mint_a,
        mint_b,
    }
}

fn escrow_pda(env: &Env, seed: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[
            ESCROW_SEED,
            env.maker.pubkey().as_ref(),
            &seed.to_le_bytes(),
        ],
        &env.program,
    )
    .0
}

fn ix_make(env: &Env, seed: u64, deposit: u64, receive: u64) -> Instruction {
    let maker = env.maker.pubkey();
    let escrow = escrow_pda(env, seed);
    let mut data = MAKE.to_vec();
    for arg in [seed, deposit, receive] {
        data.extend(arg.to_le_bytes());
    }
    Instruction::new_with_bytes(
        env.program,
        &data,
        vec![
            AccountMeta::new(maker, true),
            AccountMeta::new_readonly(env.mint_a, false),
            AccountMeta::new_readonly(env.mint_b, false),
            AccountMeta::new(ata(&maker, &env.mint_a, &env.token_program), false),
            AccountMeta::new(escrow, false),
            AccountMeta::new(ata(&escrow, &env.mint_a, &env.token_program), false),
            AccountMeta::new_readonly(associated_token::ID, false),
            AccountMeta::new_readonly(env.token_program, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
    )
}

fn ix_take(env: &Env, seed: u64, taker: &Pubkey, mint_b: &Pubkey) -> Instruction {
    let maker = env.maker.pubkey();
    let escrow = escrow_pda(env, seed);
    Instruction::new_with_bytes(
        env.program,
        &TAKE,
        vec![
            AccountMeta::new(*taker, true),
            AccountMeta::new(maker, false),
            AccountMeta::new_readonly(env.mint_a, false),
            AccountMeta::new_readonly(*mint_b, false),
            AccountMeta::new(ata(taker, &env.mint_a, &env.token_program), false),
            AccountMeta::new(ata(taker, mint_b, &env.token_program), false),
            AccountMeta::new(ata(&maker, mint_b, &env.token_program), false),
            AccountMeta::new(escrow, false),
            AccountMeta::new(ata(&escrow, &env.mint_a, &env.token_program), false),
            AccountMeta::new_readonly(associated_token::ID, false),
            AccountMeta::new_readonly(env.token_program, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
    )
}

fn ix_refund(env: &Env, seed: u64, signer: &Pubkey) -> Instruction {
    let escrow = escrow_pda(env, seed);
    Instruction::new_with_bytes(
        env.program,
        &REFUND,
        vec![
            AccountMeta::new(*signer, true),
            AccountMeta::new_readonly(env.mint_a, false),
            AccountMeta::new(ata(signer, &env.mint_a, &env.token_program), false),
            AccountMeta::new(escrow, false),
            AccountMeta::new(ata(&escrow, &env.mint_a, &env.token_program), false),
            AccountMeta::new_readonly(associated_token::ID, false),
            AccountMeta::new_readonly(env.token_program, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
    )
}

fn make(env: &mut Env, seed: u64, deposit: u64, receive: u64) {
    let ix = ix_make(env, seed, deposit, receive);
    send(&mut env.svm, &[ix], &env.maker, &[&env.maker]).unwrap();
}

#[test]
fn make_locks_tokens_in_a_pda_owned_vault() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let maker = env.maker.pubkey();
        make(&mut env, 1, 400, 250);

        let escrow = escrow_pda(&env, 1);
        let vault = ata(&escrow, &env.mint_a, &tp);
        assert_eq!(balance(&env.svm, &vault), 400);
        assert_eq!(balance(&env.svm, &ata(&maker, &env.mint_a, &tp)), 600);

        let state = Escrow::unpack(&env.svm.get_account(&escrow).unwrap().data).unwrap();
        assert_eq!(
            (state.seed, state.maker.to_bytes(), state.receive),
            (1, maker.to_bytes(), 250)
        );
        assert_eq!(
            (state.mint_a.to_bytes(), state.mint_b.to_bytes()),
            (env.mint_a.to_bytes(), env.mint_b.to_bytes())
        );
    }
}

#[test]
fn take_swaps_atomically_and_closes_everything() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let (maker, taker) = (env.maker.pubkey(), env.taker.pubkey());
        make(&mut env, 7, 400, 250);
        let escrow = escrow_pda(&env, 7);
        let vault = ata(&escrow, &env.mint_a, &tp);
        let locked_rent =
            env.svm.get_balance(&escrow).unwrap() + env.svm.get_balance(&vault).unwrap();
        let maker_sol_before = env.svm.get_balance(&maker).unwrap();

        let ix = ix_take(&env, 7, &taker, &env.mint_b);
        send(&mut env.svm, &[ix], &env.taker, &[&env.taker]).unwrap();

        assert_eq!(
            balance(&env.svm, &ata(&taker, &env.mint_a, &tp)),
            400,
            "taker got the vault"
        );
        assert_eq!(
            balance(&env.svm, &ata(&taker, &env.mint_b, &tp)),
            750,
            "taker paid 250 B"
        );
        assert_eq!(
            balance(&env.svm, &ata(&maker, &env.mint_b, &tp)),
            250,
            "maker got 250 B"
        );
        assert!(is_closed(&env.svm, &escrow) && is_closed(&env.svm, &vault));
        // The taker paid the tx fee and the new ATAs; the maker gets back exactly the rent they locked.
        assert_eq!(
            env.svm.get_balance(&maker).unwrap(),
            maker_sol_before + locked_rent
        );
    }
}

#[test]
fn refund_returns_the_vault_and_closes_everything() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let maker = env.maker.pubkey();
        make(&mut env, 3, 400, 250);

        let ix = ix_refund(&env, 3, &maker);
        send(&mut env.svm, &[ix], &env.maker, &[&env.maker]).unwrap();

        let escrow = escrow_pda(&env, 3);
        assert_eq!(balance(&env.svm, &ata(&maker, &env.mint_a, &tp)), 1_000);
        assert!(
            is_closed(&env.svm, &escrow) && is_closed(&env.svm, &ata(&escrow, &env.mint_a, &tp))
        );
    }
}

#[test]
fn one_maker_can_run_several_escrows() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let maker = env.maker.pubkey();
        make(&mut env, 1, 100, 10);
        make(&mut env, 2, 200, 20);
        assert_ne!(escrow_pda(&env, 1), escrow_pda(&env, 2));

        let ix = ix_take(&env, 2, &env.taker.pubkey(), &env.mint_b);
        send(&mut env.svm, &[ix], &env.taker, &[&env.taker]).unwrap();
        assert_eq!(
            balance(&env.svm, &ata(&escrow_pda(&env, 1), &env.mint_a, &tp)),
            100,
            "escrow 1 untouched"
        );
        // The first take created the output ATAs. The second reuses them.
        let taker = env.taker.pubkey();
        let ix = ix_take(&env, 1, &taker, &env.mint_b);
        send(&mut env.svm, &[ix], &env.taker, &[&env.taker]).unwrap();
        assert_eq!(balance(&env.svm, &ata(&taker, &env.mint_a, &tp)), 300);
        assert_eq!(balance(&env.svm, &ata(&maker, &env.mint_b, &tp)), 30);
    }
}

#[test]
fn only_the_maker_can_refund() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        make(&mut env, 5, 400, 250);

        let attacker = env.taker.insecure_clone();
        let ix = ix_refund(&env, 5, &attacker.pubkey());
        let logs = send(&mut env.svm, &[ix], &attacker, &[&attacker]).unwrap_err();
        assert!(
            logs.contains("custom program error: 0x1772"),
            "unexpected logs:\n{logs}"
        );
        assert_eq!(
            balance(&env.svm, &ata(&escrow_pda(&env, 5), &env.mint_a, &tp)),
            400
        );
    }
}

#[test]
fn take_rejects_a_different_payment_mint() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        make(&mut env, 9, 400, 250);

        // The taker mints a worthless token and tries to pay with it.
        let taker = env.taker.insecure_clone();
        let fake_b = create_mint(&mut env.svm, &Keypair::new(), &taker, &tp);
        fund(&mut env.svm, &taker, &taker.pubkey(), &fake_b, &tp, 1_000);

        let ix = ix_take(&env, 9, &taker.pubkey(), &fake_b);
        let logs = send(&mut env.svm, &[ix], &taker, &[&taker]).unwrap_err();
        assert!(
            logs.contains("custom program error: 0x1774"),
            "unexpected logs:\n{logs}"
        );
        assert_eq!(
            balance(&env.svm, &ata(&escrow_pda(&env, 9), &env.mint_a, &tp)),
            400
        );
    }
}

#[test]
fn take_fails_atomically_if_the_taker_cannot_pay() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        make(&mut env, 4, 400, 5_000); // asks for more B than the taker holds

        let ix = ix_take(&env, 4, &env.taker.pubkey(), &env.mint_b);
        assert!(send(&mut env.svm, &[ix], &env.taker, &[&env.taker]).is_err());

        // Nothing moved: the vault is still full and the taker still has all their B.
        let escrow = escrow_pda(&env, 4);
        assert_eq!(balance(&env.svm, &ata(&escrow, &env.mint_a, &tp)), 400);
        assert_eq!(
            balance(&env.svm, &ata(&env.taker.pubkey(), &env.mint_b, &tp)),
            1_000
        );
    }
}

#[test]
fn make_rejects_zero_amounts_and_same_mint() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);

        for (deposit, receive) in [(0, 10), (10, 0)] {
            let ix = ix_make(&env, 1, deposit, receive);
            let logs = send(&mut env.svm, &[ix], &env.maker, &[&env.maker]).unwrap_err();
            assert!(
                logs.contains("custom program error: 0x1770"),
                "unexpected logs:\n{logs}"
            );
        }

        env.mint_b = env.mint_a;
        let ix = ix_make(&env, 1, 10, 10);
        let logs = send(&mut env.svm, &[ix], &env.maker, &[&env.maker]).unwrap_err();
        assert!(
            logs.contains("custom program error: 0x1771"),
            "unexpected logs:\n{logs}"
        );
    }
}

#[test]
fn prefunded_escrow_can_be_created_and_duplicate_make_preserves_it() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let escrow = escrow_pda(&env, 1);
        let prefund = system_instruction::transfer(&env.maker.pubkey(), &escrow, 1);
        send(&mut env.svm, &[prefund], &env.maker, &[&env.maker]).unwrap();
        make(&mut env, 1, 400, 250);
        let before = env.svm.get_account(&escrow).unwrap();

        let duplicate = ix_make(&env, 1, 500, 10);
        assert!(send(&mut env.svm, &[duplicate], &env.maker, &[&env.maker]).is_err());
        assert_eq!(env.svm.get_account(&escrow).unwrap(), before);
        assert_eq!(balance(&env.svm, &ata(&escrow, &env.mint_a, &tp)), 400);
    }
}

#[test]
fn refund_recreates_closed_maker_ata() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let maker = env.maker.pubkey();
        let source = ata(&maker, &env.mint_a, &tp);
        make(&mut env, 1, 1_000, 250);
        let close =
            spl_token_2022::instruction::close_account(&tp, &source, &maker, &maker, &[]).unwrap();
        send(&mut env.svm, &[close], &env.maker, &[&env.maker]).unwrap();
        assert!(is_closed(&env.svm, &source));

        let refund = ix_refund(&env, 1, &maker);
        send(&mut env.svm, &[refund], &env.maker, &[&env.maker]).unwrap();
        assert_eq!(balance(&env.svm, &source), 1_000);
    }
}

#[test]
fn take_rolls_back_payment_and_new_atas_if_vault_transfer_fails() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let (maker, taker) = (env.maker.pubkey(), env.taker.pubkey());
        make(&mut env, 1, 400, 250);
        let escrow = escrow_pda(&env, 1);
        let vault = ata(&escrow, &env.mint_a, &tp);
        let freeze =
            spl_token_2022::instruction::freeze_account(&tp, &vault, &env.mint_a, &maker, &[])
                .unwrap();
        send(&mut env.svm, &[freeze], &env.maker, &[&env.maker]).unwrap();

        let take = ix_take(&env, 1, &taker, &env.mint_b);
        let logs = send(&mut env.svm, &[take], &env.taker, &[&env.taker]).unwrap_err();
        assert_eq!(
            logs.matches("Instruction: TransferChecked").count(),
            2,
            "{logs}"
        );
        assert_eq!(balance(&env.svm, &ata(&taker, &env.mint_b, &tp)), 1_000);
        assert_eq!(balance(&env.svm, &vault), 400);
        assert!(!is_closed(&env.svm, &escrow));
        assert!(is_closed(&env.svm, &ata(&maker, &env.mint_b, &tp)));
        assert!(is_closed(&env.svm, &ata(&taker, &env.mint_a, &tp)));
    }
}

fn measure(token: Pubkey, binary: &[u8], seed: u64) -> ([u64; 3], Vec<u8>) {
    // Independent VMs let the programs use the same ID and bump searches.
    let mut env = setup_program(token, ANCHOR_ID, binary);
    let (maker, taker) = (env.maker.pubkey(), env.taker.pubkey());
    let make = ix_make(&env, seed, 400, 250);
    let open = send(&mut env.svm, &[make], &env.maker, &[&env.maker]).unwrap();
    let state = env.svm.get_account(&escrow_pda(&env, seed)).unwrap().data;
    let take = ix_take(&env, seed, &taker, &env.mint_b);
    let accepted = send(&mut env.svm, &[take], &env.taker, &[&env.taker]).unwrap();
    let refund_seed = seed + 1;
    let make_again = ix_make(&env, refund_seed, 100, 50);
    send(&mut env.svm, &[make_again], &env.maker, &[&env.maker]).unwrap();
    let refund = ix_refund(&env, refund_seed, &maker);
    let cancelled = send(&mut env.svm, &[refund], &env.maker, &[&env.maker]).unwrap();
    ([open, accepted, cancelled], state)
}

#[test]
fn compare_compute_units() {
    const SAMPLES: u64 = 16;
    println!("| Token program | Instruction | Anchor | Anchor (no logs) | Pinocchio | Savings vs no-log Anchor |");
    println!("|---|---|---:|---:|---:|---:|");
    for (token, label) in TOKEN_PROGRAMS.into_iter().zip(["Token", "Token-2022"]) {
        let mut totals = [[0; 3]; 3];
        for sample in 0..SAMPLES {
            let seed = sample * 2;
            let (anchor, anchor_state) = measure(token, ANCHOR_SO, seed);
            let (no_log, _) = measure(token, ANCHOR_NO_LOG_SO, seed);
            let (pinocchio, pinocchio_state) = measure(token, PINOCCHIO_SO, seed);
            assert_eq!(anchor_state, pinocchio_state);
            for (total, units) in totals.iter_mut().zip([anchor, no_log, pinocchio]) {
                for (sum, cu) in total.iter_mut().zip(units) {
                    *sum += cu;
                }
            }
        }
        for (index, instruction) in ["make", "take", "refund"].into_iter().enumerate() {
            let [anchor, no_log, pinocchio] = totals.map(|total| total[index] / SAMPLES);
            let savings = (1.0 - pinocchio as f64 / no_log as f64) * 100.0;
            println!(
                "| {label} | {instruction} | {anchor} | {no_log} | {pinocchio} | {savings:.1}% |"
            );
            assert!(pinocchio < no_log, "{label} {instruction}");
        }
    }
    println!(
        "Binary size: {} / {} / {} bytes (Anchor / Anchor no logs / Pinocchio)",
        ANCHOR_SO.len(),
        ANCHOR_NO_LOG_SO.len(),
        PINOCCHIO_SO.len()
    );
}
