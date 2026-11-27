# Pinocchio token escrow

[![CI](https://github.com/malharc373/solana-pinocchio-escrow/actions/workflows/ci.yml/badge.svg)](https://github.com/malharc373/solana-pinocchio-escrow/actions/workflows/ci.yml)

A Pinocchio 0.12 port of [solana-escrow](https://github.com/malharc373/solana-escrow). A maker deposits token A into a PDA-owned vault and sets a price in token B. A taker pays that price and receives the vault balance atomically, or the maker refunds the offer. Both settlement paths close the vault and the escrow state and return their lamports to the maker.

## Build and test

Use Solana CLI 3.1.10 and Rust 1.89.

```sh
cargo fmt --check
cargo build-sbf -- --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --test escrow -- --nocapture
```

The LiteSVM suite ports all eight scenarios from the Anchor project and runs each with the Token and Token-2022 programs. It also covers pre-funded PDAs, duplicate initialization, recreation of the maker's ATA, and rollback when the vault transfer fails after payment.

## Client interface

The port preserves the Anchor instruction discriminators, account order and flags, and 121-byte escrow state layout. To reuse the [Codama client](https://github.com/malharc373/solana-escrow/tree/main/client) with a separate deployment, pass that deployment's program ID to its instruction builders and use it when deriving the escrow PDA. The seeds are `["escrow", maker, seed.to_le_bytes()]`. The vault is the escrow PDA's associated token account for mint A. `make` takes three little-endian `u64` values in this order: `seed`, `deposit`, `receive`. `take` and `refund` have no arguments.

Both mints must use the same token program. Amounts are in base units. The port retains the Anchor version's checked-transfer behavior. Transfer fees can reduce the net amount received, and extensions that require additional CPI accounts, such as transfer hooks, are outside this interface.

## Compute units

Mean whole-transaction CU from LiteSVM 0.10.0. Each row averages 16 fixed seed pairs. All three binaries run with the same program ID, keys, addresses, token instructions and rent schedule in independent VMs. Each `take` creates both output ATAs. The benchmark also asserts that the two implementations write identical escrow state bytes.

| Token program | Instruction | Anchor | Anchor (no instruction logs) | Pinocchio | CU saved vs no-log Anchor |
|---|---|---:|---:|---:|---:|
| Token | `make` | 50,263 | 49,322 | 36,893 | 25.2% |
| Token | `take` | 96,320 | 95,775 | 69,202 | 27.7% |
| Token | `refund` | 26,347 | 25,806 | 16,614 | 35.6% |
| Token-2022 | `make` | 45,457 | 44,516 | 31,491 | 29.3% |
| Token-2022 | `take` | 86,482 | 85,937 | 53,322 | 38.0% |
| Token-2022 | `refund` | 23,709 | 23,168 | 13,953 | 39.8% |

Binary size was 262,952 bytes for Anchor, 261,560 for Anchor with instruction logs disabled and 30,864 for Pinocchio.

The fixtures in `bench/` were built from [solana-escrow at d83633e](https://github.com/malharc373/solana-escrow/tree/d83633ebd29031d5015efa2a9677cdce798bc174), whose lockfile resolves Anchor 1.2.2. Build that commit with Solana CLI 3.1.10 using `cargo build-sbf -- --locked`. To reproduce the second fixture, remove the `msg!` calls in the three instruction handlers and rebuild with `cargo build-sbf --features no-log-ix-name -- --locked`.
