# Balance-based settlement instead of a pairwise debt ledger

Heimdall kept a running debt for every pair of members, so a Payment only reduced the debt between its Sender and Recipient, and following a "simplified" suggestion could make the numbers worse. Asgard tracks only each Member's Balance (Expenses and Payments shift it) and derives Settlements: the fewest Payments that would bring every Balance to zero. The goal is "what do I send to whom so we're even" between three friends, not inter-member accounting, so the pairwise history isn't worth its confusion.

## Consequences

- A Payment to anyone counts toward settling up; the plan is recomputed from Balances and never goes stale.
- The app can no longer answer "how much did A ever owe B specifically".
- Heimdall import is verified by comparing per-Member Balances, not pairwise debts.
