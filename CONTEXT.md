# Asgard

The shared app of a small home-brewery crew: their common money, shopping, brewery log, reminders and quotes in one place.

## Names

**Runa**:
The brewery: the real-world thing the Crew runs, with its own public site outside Asgard. Asgard's logo and icons are Runa's.
_Avoid_: Asgard (that's the app)

**Asgard**:
The Crew's private app at `asgard.joseforge.cz`. It is for Members only and is not the brewery's public face.
_Avoid_: Runa (that's the brewery), the site

## People

**Crew**:
The closed group of people who brew together and share costs. There is exactly one Crew.
_Avoid_: brewery (that's Runa, not the group), guild, server

**Member**:
A person in the Crew.
_Avoid_: user, slot, viking

**Admin**:
A Member who may also correct any other Member's entries and generate Invite and Device links.
_Avoid_: owner, moderator

**Invite link**:
A one-time link that lets a new person join the Crew as a Member.
_Avoid_: slot, assign

**Device link**:
A one-time link that lets an existing Member sign in on a new device when no passkey is available.
_Avoid_: recovery code, reset link

## Money

**Expense**:
Money a Member spent on behalf of the Crew, shared equally by every Member who was in the Crew on its Expense date.
_Avoid_: purchase, cost, transfer

**Payer**:
The Member who spent the money for an Expense.
_Avoid_: owner, buyer

**Expense date**:
The day the money was actually spent, which may differ from when the Expense was logged.

**Share**:
One Member's equal part of an Expense, in whole crowns. The Payer absorbs any rounding remainder.

**Payment**:
Money sent directly from one Member (the **Sender**) to another (the **Recipient**) to settle up.
_Avoid_: transfer, repayment

**Entry**:
An Expense or a Payment, as the Crew logs, edits and deletes them. Only its Payer or Sender, or the Admin, may change an Entry.
_Avoid_: transaction, record

**Logger**:
The Member who entered an Expense or Payment into Asgard, who may differ from its Payer or Sender.
_Avoid_: creator, author

**Balance**:
A Member's net position across all Expenses and Payments: positive means the Crew owes them, negative means they owe the Crew.
_Avoid_: stats, total

**Settlement**:
One suggested Payment ("X sends Y this much") from the smallest set of Payments that would bring every Balance to zero.
_Avoid_: debt, owing, pairwise debt

**Even**:
The state where a Member's Balance is within about a crown of zero.
_Avoid_: kvit, settled

**History**:
Every Entry that hasn't been deleted, newest first by its Expense date or the day a Payment was logged on, filterable by a Member (as Payer, Sender or Recipient) and a calendar month in Prague.
_Avoid_: log, ledger, transactions

**Monthly total**:
What the whole Crew's Expenses, and separately its Payments, added up to in one calendar month. It says what the brewery cost, not what anyone owes, so it ignores the Member filter.
_Avoid_: Balance, stats

## Shopping

**Shopping list**:
The Crew's shared list of things that need to be bought.
_Avoid_: todo, task list

**Item**:
One thing on the Shopping list, either **open** or **bought**.
_Avoid_: todo, task

**Urgency**:
How soon an Item is needed: *ASAP*, *Next brewday* or *Nice to have*. *Next brewday* means by the next Brewing slot.
_Avoid_: priority

**Recently bought**:
The Items bought in the last 7 days, where marking one bought can be undone. Older bought Items are kept but not shown.
_Avoid_: done, history

## Brewing

**Batch**:
One brew, logged in the Batches tab: a name, free-text Notes, and later its Brew day, Bottling and Recipe. Any Member may add, edit or delete any Batch.
_Avoid_: brew log entry (Entry means an Expense or Payment), recipe (that's the Recipe a Batch may carry)

**Batch number**:
A Batch's automatic number, #1, #2, …: one more than the highest ever used, and never used again after its Batch is deleted, so a number always means the same brew.
_Avoid_: id

**Recipe**:
The BeerXML file a Batch may carry, exported from Brewfather, BeerSmith or similar and kept exactly as it was uploaded. Optional, and can be attached, replaced or removed later.
_Avoid_: recipe for the Batch itself

**Recipe viewer**:
The light, read-only view of a Batch's Recipe: style, targets, fermentables, hops, yeast and mash steps, in metric units.

**Brewing slot** (short: **slot**):
A date the Crew has set aside for brewing, holding at most one Planned Batch.
_Avoid_: Brew day (that's what was recorded on a Batch), session, event

**Brew day**:
When a Batch was brewed: the brew date, its OG and its Volume.

**Bottling**:
When a Batch was bottled: the bottling date and its FG.

**OG**:
Original gravity: the specific gravity measured on Brew day, such as 1.052.

**FG**:
Final gravity: the specific gravity measured at the end of fermentation, such as 1.010. Always below the OG.

**Volume**:
How many litres went into the fermenter on Brew day, to one decimal.

**ABV**:
Alcohol by volume, worked out from OG and FG and never entered by hand.

**Attenuation**:
Apparent attenuation: how much of the sugar the yeast ate, `(OG − FG) / (OG − 1)`, worked out alongside the ABV and never entered by hand.

**Stage**:
Where a Batch is: *Planned*, *Brewed* (shown as "In the fermenter") or *Bottled*. It follows from which dates are recorded, so no Member sets it.
_Avoid_: status

**Notes**:
Free text on a Batch, with its line breaks kept: what went wrong, dry hopping, tasting impressions.

## Reminders

**Reminder**:
A text the whole Crew is notified about at a given date and time. It is either **pending** or **sent**.
_Avoid_: notification, alert

**Quick time**:
A common due time the Reminder form fills in with one tap, worked out from now in Prague: *+1 hour*, *Tomorrow 9:00* or *+1 week*.
_Avoid_: preset, shortcut

**Notification**:
A push message shown on a Member's subscribed device, telling them about a due Reminder, an Expense or Payment that involves them, an *ASAP* Item, or a Brewing slot another Member just planned. Tapping it opens the tab it is about.
_Avoid_: alert, message

## Quotes

**Quote**:
Something memorable someone said, kept in the Crew's collection.

**Author**:
Whoever said a Quote, written as free text; not necessarily a Member.
_Avoid_: speaker

**Added by**:
The Member who put a Quote into the collection.
