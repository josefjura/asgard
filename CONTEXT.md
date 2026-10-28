# Asgard

The shared app of a small home-brewery crew: their common money, shopping, reminders and quotes in one place.

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

## Shopping

**Shopping list**:
The Crew's shared list of things that need to be bought.
_Avoid_: todo, task list

**Item**:
One thing on the Shopping list, either **open** or **bought**.
_Avoid_: todo, task

**Urgency**:
How soon an Item is needed: *ASAP*, *Next brewday* or *Nice to have*.
_Avoid_: priority

## Reminders

**Reminder**:
A text the whole Crew is notified about at a given date and time. It is either **pending** or **sent**.
_Avoid_: notification, alert

## Quotes

**Quote**:
Something memorable someone said, kept in the Crew's collection.

**Author**:
Whoever said a Quote, written as free text; not necessarily a Member.
_Avoid_: speaker

**Added by**:
The Member who put a Quote into the collection.
