# Oxidized Transactions
Simple toy payments engine written in Rust that reads a series of transactions
from a CSV, updates client accounts, handles disputes and chargebacks, and then outputs the
state of clients accounts as a CSV.

# Cases to Test
A bunch of test cases to test for one account at a time.
Once all test cases pass, then add in multiple clients in the file

## Deposits only
Ensure total amount is meets expected.

## Withdrawals only
1. Start with massive deposit and withdraw money
2. Start with small deposit and withdraw money until insufficient funds

## Dispute
1. Single deposit, single withdrawal, dispute the withdrawal
2. Single deposit, single withdrawal, dispute with nonexistent transaction ID
3. Test scenario 1, but with 0 available funds

## Resolve
1. Single deposit, single withdrawal, dispute the withdrawal, resolve dispute 
2. Single deposit, single withdrawal, dispute the withdrawal, resolve dispute with nonexistent transaction ID
3. Test scenario 1, but with 0 available funds at dispute, resolved with funds returned

## Chargeback
1. Single deposit, dispute the deposit, chargeback -- freeze account
