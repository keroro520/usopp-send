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

```markdown
### Transaction Summary Table

| RPC | Sent Duration(Client->Node) | Tx Full Signature | Tx Status |
|---|---|---|---|
| https://api.zan.top/node/v1/solana/devnet/ebd0bceef25b4df1b3cd2aa3b7d76725 | 436ms | 5sPBpY1aS7TooRuXyDH5vTWdZsLCWwxjyGJPzidWx1RftosEPWKoxqq8sus4S4k5womb3FRbeQ6cUd4eZS6kGLug | 🏆 Confirmed (6370ms) |
| https://few-stylish-sanctuary.solana-devnet.quiknode.pro/9da60d3851f306c2f6ffee306fd02cfa37e2e244 | 859ms | 4Pwusa7nssqvc9y7Z57TNM2bpFTHYJrBmSZeUJbUz92SZHYmdg5Y8yA4CRjwRRLwot8VouaDNy5ATaUQGwXkrwnY | Failed on-chain: InstructionError(0, Custom(1)) |
| https://solana-devnet.g.alchemy.com/v2/nrWk_B4gDDCY_lvRbKPG-OzcXn40FBhG | 6011ms | 4qj7udFFvycocATovbXvsEHCWCoZYDPkJAVLLZQ2zNYdzyqhp2WVcvuY36mWTiVpUWPiYzEF5yCgRQ8EjAPFyaWu | Failed on-chain: InstructionError(0, Custom(1)) |
| https://devnet.helius-rpc.com/?api-key=62d4baa9-f668-4311-a736-b21fea80169e | 5484ms | 5NP57wwCyRAbLzyEudgjqnppBpkNv8A6hds2fx3u1CijMEBvCsigfBppk8XXaFxKteM1vjJA65FZnnyHXjHhzNd5 | Failed on-chain: InstructionError(0, Custom(1)) |
```
