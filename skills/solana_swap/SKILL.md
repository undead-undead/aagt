---
name: solana_swap
description: Swap tokens on Solana using Raydium or Jupiter.
parameters:
  type: object
  properties:
    from_token:
      type: string
      description: Token to sell (e.g., SOL, USDC)
    to_token:
      type: string
      description: Token to buy
    amount:
      type: string
      description: Amount to swap (e.g., "0.1", "10%")
  required: [from_token, to_token, amount]
script: swap.py
runtime: python3
---

# Solana Swap Skill

This skill allows you to swap tokens on the Solana blockchain.
It uses a Python script to calculate the best route and then returns a proposal to AAGT.

## Safety Rules
- Never swap if slippage is > 5%.
- Always check that the 'to_token' is not a known scam token.
