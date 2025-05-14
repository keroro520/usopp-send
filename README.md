# Usopp-Send

Usopp-Send is a tool for comparing transaction propagation speed across different Solana RPC nodes by sending mutually conflicting transactions and tracking which one confirms first.

## Project Overview

Different RPC nodes can have varying transaction propagation speeds due to differences in geographic location, network topology, and server load. Usopp-Send helps identify which RPC nodes provide the fastest path to transaction confirmation by quantifying their "path quality."

The tool works by:
1. Constructing multiple conflicting transactions (using the same sender account)
2. Sending each transaction through a different RPC node simultaneously
3. Monitoring which transaction confirms first
4. Reporting detailed metrics about transaction confirmation times

## Key Features

- **Fair Transaction Dispatch**: Uses a two-phase approach with system threads and oneshot channels to ensure transactions are sent simultaneously
- **Conflicting Transaction Construction**: Creates transactions that transfer decreasing percentages of the sender's balance

## Example

```
cargo run --release
```

Generated report:

```markdown
### Transaction Summary Table
| RPC | Tx Status | Sent Duration | Tx Full Signature |
|---|---|---|---|
| rpc1 | 🏆 Confirmed (6452ms) | 411ms | 3ycPvx5CnT6FxEFnPo7P2jAK2UEc9oC25acgAVDYeXmKntmPZkkHV3KsdVGkJgHDAWtfRHGuWQNDLRJLw64PsUW |
| rpc2 | Failed on-chain: InstructionError(0, Custom(1)) | 5501ms | 29eJchXctdteZ4P9tWCA9DTQxAz4vwwEisCKYBiHGCYgQDD96r23QaSpZb2Zs7WUxofYvvYdLiXv2jHERw78axR1 |
| rpc3 | Failed on-chain: InstructionError(0, Custom(1)) | 5989ms | 4E2gs7qo3kFqJbf84F41PfhcZQLnGArTHddwC29XzEpuYDAJmiMMPJCp4eGCpref1SUKkcGxtjZrHoobtNy2FWuk |
| rpc4 | Failed on-chain: InstructionError(0, Custom(1)) | 969ms | 5zpwuK8uLQson4PgQEAHZkx5sfA9rtQSwQz86kJB7Jj9k2NRXmXW1H2aRHXYZM3YekWtrue4CpDCoCJunDAZnZs6 |
```
