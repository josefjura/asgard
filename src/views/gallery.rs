//! `/dev/gallery`: every component with fixture data in all its states, at phone and desktop
//! width. Compiled into debug builds only.

use maud::{DOCTYPE, Markup, html};

use crate::assets;
use crate::domain::auth::{LinkKind, PasskeyName};
use crate::domain::batches::Stage;
use crate::domain::finance::history::HistoryFilter;
use crate::domain::members::{CREW_NAME, DisplayName};
use crate::views::admin::{self, Outcome as AdminOutcome};
use crate::views::batches::{self, BatchProblems, BatchValues, Purpose as BatchPurpose};
use crate::views::brand::header_logo;
use crate::views::display_name::{self, Outcome};
use crate::views::finance::{
    self, EntryPage, ExpenseProblems, ExpenseValues, PaymentProblems, PaymentValues, Purpose,
};
use crate::views::history::{self, Filters, NOT_A_MONTH};
use crate::views::layout::document;
use crate::views::nav::{ADMIN_HREF, Place, Tab, bottom_nav};
use crate::views::passkey::CeremonyUrls;
use crate::views::passkeys::{self, Outcome as PasskeyOutcome};
use crate::views::push::{self, PushState, PushUrls};
use crate::views::quotes::{self, Purpose as QuotePurpose, QuoteProblems, QuoteValues};
use crate::views::reminders::{self, ReminderProblems, ReminderValues};
use crate::views::shopping::{self, Fold, ItemProblems, ItemValues};
use crate::views::slots;
use crate::views::{invite, offline, settings, sign_in, tabs, welcome};

/// One component in one state, rendered as a complete document.
pub struct Fixture {
    pub component: &'static str,
    pub state: String,
    pub document: Markup,
    /// How tall the frames need to be to show the whole fixture, at both widths.
    pub height: u16,
}

const PHONE_WIDTH: u16 = 390;
const PAGE_HEIGHT: u16 = 720;
/// The welcome page is long: its sections stack on a phone.
const WELCOME_HEIGHT: u16 = 1500;
/// Settings stacks its sections under the app shell's header.
const SETTINGS_HEIGHT: u16 = 1150;
/// Each extra passkey adds a card to the settings page.
const SEVERAL_PASSKEYS_HEIGHT: u16 = 1500;
/// The Admin's settings have one more section.
const ADMIN_SETTINGS_HEIGHT: u16 = 1300;
/// The admin page's two sections, with a new link or several open ones.
const ADMIN_HEIGHT: u16 = 1000;
/// The Finance home screen: a Balance with whom to send how much, the buttons, four entries, and
/// the Crew's Balances with the Settlement plan.
const FINANCE_HEIGHT: u16 = 1500;
/// The Expense form with a problem under every field, or with the delete button beneath it.
const EXPENSE_FORM_HEIGHT: u16 = 900;
/// The Payment form with its problems, or with the delete button beneath it.
const PAYMENT_FORM_HEIGHT: u16 = 860;
/// An entry row with the edited line and the edit and delete controls under it.
const ENTRY_ROW_HEIGHT: u16 = 180;
/// History: the filters, four entries, the pages and three months' totals.
const HISTORY_HEIGHT: u16 = 1250;
/// History filtered down to two entries and one month.
const FILTERED_HISTORY_HEIGHT: u16 = 950;
/// Where the fixture passkey lists' forms post.
const PASSKEYS_BASE: &str = "/settings/passkeys";
/// What the fixture notification controls hand the push script.
const PUSH_URLS: PushUrls<'static> = PushUrls {
    public_key: "BFixtureKey",
    subscribe: "/push/subscribe",
    unsubscribe: "/push/unsubscribe",
};
/// The notifications control, alone in a section.
const PUSH_CONTROL_HEIGHT: u16 = 300;
/// The Reminders tab: the form, and three pending and two sent Reminders under it on a phone.
const REMINDERS_HEIGHT: u16 = 1100;
/// A Reminder's edit page: the form, and delete beneath it.
const REMINDER_EDIT_HEIGHT: u16 = 700;
/// The Shopping tab: the form, five Items in three groups under it on a phone, and Recently
/// bought folded away beneath.
const SHOPPING_HEIGHT: u16 = 1400;
/// An open Item with a two-line note.
const ITEM_ROW_HEIGHT: u16 = 160;
/// Recently bought unfolded, with three Items, one with a long name.
const RECENTLY_BOUGHT_HEIGHT: u16 = 420;
/// The Item form with its note open and a problem under it.
const ITEM_FORM_HEIGHT: u16 = 560;
/// The Quotes tab with nothing in the collection yet.
const QUOTES_HEIGHT: u16 = 900;
/// The Quotes tab with a random Quote and four in the collection, one long, on a phone.
const QUOTES_COLLECTION_HEIGHT: u16 = 1500;
/// The random-Quote card with a Quote that breaks across lines.
const RANDOM_CARD_HEIGHT: u16 = 260;
/// A Quote row whose text wraps, or breaks across lines.
const QUOTE_ROW_HEIGHT: u16 = 280;
/// The add form with a problem under each field.
const QUOTE_FORM_HEIGHT: u16 = 700;
/// The Batches tab: three Brewing slots and the plan form, then the add form with its Recipe
/// field, and three Batches under it on a phone.
const BATCHES_HEIGHT: u16 = 1600;
/// A Batch's page with its Brew day, Bottling, Recipe upload and Notes over several lines.
const BATCH_PAGE_HEIGHT: u16 = 1450;
/// A Batch's page with the whole Recipe viewer, on a phone.
const RECIPE_PAGE_HEIGHT: u16 = 2900;
/// The whole Recipe viewer on its own, on a phone.
const RECIPE_VIEWER_HEIGHT: u16 = 1500;
/// The Batch form with a problem under each field, or with Cancel and delete beneath it.
const BATCH_FORM_HEIGHT: u16 = 900;
/// The Brewing slots section alone, with the most slots its fixtures have.
const SLOTS_HEIGHT: u16 = 520;
const DESKTOP_WIDTH: u16 = 1280;

/// # Panics
///
/// If a fixture's display name is invalid, which is a bug in the fixtures.
#[must_use]
pub fn fixtures() -> Vec<Fixture> {
    let nav = Tab::ALL.into_iter().map(|tab| Fixture {
        component: "Bottom navigation",
        state: format!("{} active", tab.label()),
        document: document("Bottom navigation", &bottom_nav(Place::Tab(tab))),
        height: 80,
    });
    let pages = Tab::ALL.into_iter().map(|tab| Fixture {
        component: "App shell",
        state: format!("{} tab", tab.label()),
        document: tabs::page(tab),
        height: PAGE_HEIGHT,
    });
    let offline = Fixture {
        component: "Offline page",
        state: "No network".to_owned(),
        document: offline::page(),
        height: PAGE_HEIGHT,
    };
    let name = |name: &str| DisplayName::parse(name).expect("fixture names are valid");
    let invite = Fixture {
        component: "Invite page",
        state: "Valid link".to_owned(),
        document: invite::page(
            CREW_NAME,
            &name("Ondra"),
            &CeremonyUrls {
                start: "/invite/fixture/start".to_owned(),
                finish: "/invite/fixture/finish".to_owned(),
            },
        ),
        height: PAGE_HEIGHT,
    };
    let link_errors = [
        ("Naming the Admin", Some(name("Josef"))),
        ("Before there is an Admin", None),
    ]
    .map(|(state, admin)| Fixture {
        component: "Link error page",
        state: state.to_owned(),
        document: invite::link_error(admin.as_ref()),
        height: PAGE_HEIGHT,
    });
    let signed_in = Fixture {
        component: "Invite page",
        state: "Already signed in".to_owned(),
        document: invite::signed_in(&name("Josef"), LinkKind::Invite, "/invite/fixture/log-out"),
        height: PAGE_HEIGHT,
    };
    let device = [
        (
            "Valid link",
            invite::device_page(
                CREW_NAME,
                &name("Ondra"),
                &CeremonyUrls {
                    start: "/device/fixture/start".to_owned(),
                    finish: "/device/fixture/finish".to_owned(),
                },
            ),
        ),
        (
            "Already signed in",
            invite::signed_in(&name("Josef"), LinkKind::Device, "/device/fixture/log-out"),
        ),
    ]
    .map(|(state, document)| Fixture {
        component: "Device link page",
        state: state.to_owned(),
        document,
        height: PAGE_HEIGHT,
    });
    let sign_in = Fixture {
        component: "Sign-in page",
        state: "Signed out".to_owned(),
        document: sign_in::page(&CeremonyUrls {
            start: "/sign-in/start".to_owned(),
            finish: "/sign-in/finish".to_owned(),
        }),
        height: PAGE_HEIGHT,
    };
    let brand = Fixture {
        component: "Brand",
        state: "Logos on every background".to_owned(),
        document: brand(),
        height: 780,
    };
    nav.chain(pages)
        .chain([brand, offline, invite, signed_in])
        .chain(device)
        .chain(link_errors)
        .chain([sign_in])
        .chain(signed_in_pages())
        .chain(passkey_lists())
        .chain(admin_pages())
        .chain(finance_pages())
        .chain(reminder_pages())
        .chain(shopping_pages())
        .chain(quotes_pages())
        .chain(batches_pages())
        .collect()
}

/// The admin page alone in the Crew, with open links, and after each thing the Admin can do
/// on it.
fn admin_pages() -> Vec<Fixture> {
    let open = admin::fixtures::several();
    let crew = admin::fixtures::crew();
    let karel = DisplayName::parse("Karel").expect("fixture names are valid");
    let ondra = DisplayName::parse("Ondra").expect("fixture names are valid");
    let refused = "Someone in the Crew is already called josef.";
    [
        ("Alone in the Crew", &[][..], &[][..], None),
        ("No open links", &[][..], &crew[..], None),
        ("Open links", &open[..], &crew[..], None),
        (
            "Link just made",
            &open[..],
            &crew[..],
            Some(AdminOutcome::Created {
                kind: LinkKind::Invite,
                name: &karel,
                url: admin::fixtures::URL,
            }),
        ),
        (
            "Device link just made",
            &open[..],
            &crew[..],
            Some(AdminOutcome::Created {
                kind: LinkKind::Device,
                name: &ondra,
                url: admin::fixtures::DEVICE_URL,
            }),
        ),
        (
            "Name refused",
            &[][..],
            &crew[..],
            Some(AdminOutcome::Refused {
                value: "josef",
                message: refused,
            }),
        ),
        (
            "Link revoked",
            &open[1..],
            &crew[..],
            Some(AdminOutcome::Revoked(&karel)),
        ),
    ]
    .map(|(state, open, members, outcome)| Fixture {
        component: "Admin page",
        state: state.to_owned(),
        document: admin::page(&admin::links(ADMIN_HREF, open, members, outcome)),
        height: ADMIN_HEIGHT,
    })
    .into()
}

/// The Finance home screen for a Member who owes, is owed and is Even, the Expense and Payment
/// forms, and each kind of entry on its own.
fn finance_pages() -> Vec<Fixture> {
    use finance::fixtures::{
        JOSEF, ONDRA, SIMON, TODAY, crew, edited_entry, edited_payment_entry, entry, fresh, member,
        overview, payment_entry, sends_to_two,
    };

    let homes = [
        ("Owes", JOSEF, overview()),
        ("Owed", ONDRA, overview()),
        ("Even", SIMON, overview()),
        ("Nothing logged", JOSEF, fresh()),
        ("Sends to two", JOSEF, sends_to_two()),
    ]
    .map(|(state, me, overview)| Fixture {
        component: "Finance home",
        state: state.to_owned(),
        document: finance::home(&member(me), &overview),
        height: FINANCE_HEIGHT,
    });
    let today = TODAY.to_string();
    let empty = ExpenseValues::blank(ONDRA, &today);
    let mistaken = ExpenseValues {
        amount: "12,50",
        description: "",
        payer: SIMON,
        expense_date: "2026-11-02",
    };
    let problems = ExpenseProblems {
        amount: Some("Enter the amount in whole crowns.".to_owned()),
        description: Some("Say what the money was for.".to_owned()),
        payer: Some("They weren't in the Crew yet on that date.".to_owned()),
        expense_date: Some("The Expense date can't be in the future.".to_owned()),
    };
    let editing = ExpenseValues {
        amount: "12480",
        description: "Pilsner malt, 3 sacks, and a new hydrometer",
        payer: ONDRA,
        expense_date: "2026-10-28",
    };
    let forms = [
        ("Empty", Purpose::Log, &empty, ExpenseProblems::default()),
        ("With errors", Purpose::Log, &mistaken, problems),
        (
            "Editing",
            Purpose::Edit,
            &editing,
            ExpenseProblems::default(),
        ),
    ]
    .map(|(state, purpose, values, problems)| Fixture {
        component: "Expense form",
        state: state.to_owned(),
        document: finance::expense_page(
            entry_page(purpose, "/expenses/7/delete"),
            &finance::expense_form("/expenses/7", purpose, &crew(), values, &problems, TODAY),
        ),
        height: EXPENSE_FORM_HEIGHT,
    });
    let rows = [
        ("Expense", SIMON, entry()),
        ("Payment", SIMON, payment_entry(Some("For the malt"))),
        ("Payment without a description", SIMON, payment_entry(None)),
        ("Expense its Payer may change", ONDRA, entry()),
        ("Edited", ONDRA, edited_entry()),
        ("Edited Payment", JOSEF, edited_payment_entry()),
    ]
    .map(|(state, me, entry)| Fixture {
        component: "Entry row",
        state: state.to_owned(),
        document: document(
            "Entry row",
            &html! {
                main.shell-main { ul.entry-list { (finance::entry_row(&member(me), &entry, None)) } }
            },
        ),
        height: ENTRY_ROW_HEIGHT,
    });
    homes
        .into_iter()
        .chain(forms)
        .chain(payment_forms())
        .chain(rows)
        .chain(history_pages())
        .collect()
}

/// The Reminders tab with nothing pending, with Reminders pending, with Reminders pending and
/// sent, with the due time filled in by a quick button, and with a refused form; and a
/// Reminder's edit page as it opens and refused.
fn reminder_pages() -> Vec<Fixture> {
    use reminders::fixtures::{NOW, pending, refused, sent};

    let new = |values, problems: &ReminderProblems| {
        reminders::form(reminders::Purpose::Create, values, problems, NOW)
    };
    let empty = new(ReminderValues::default(), &ReminderProblems::default());
    let quick = new(
        ReminderValues {
            text: "Bottle the stout",
            due_at: "2026-10-25T09:00",
        },
        &ReminderProblems::default(),
    );
    let (values, problems) = refused();
    let refused_new = new(values, &problems);
    let tabs = [
        ("Nothing pending", &empty, Vec::new(), Vec::new()),
        ("Pending", &empty, pending(), Vec::new()),
        ("Pending and sent", &empty, pending(), sent()),
        ("Filled in by a quick button", &quick, pending(), Vec::new()),
        ("Form with errors", &refused_new, pending(), Vec::new()),
    ]
    .map(|(state, form, pending, sent)| Fixture {
        component: "Reminders tab",
        state: state.to_owned(),
        document: reminders::page(form, &pending, &sent),
        height: REMINDERS_HEIGHT,
    });

    let editing = &pending()[2];
    let edits = [
        ("Opened", reminders::edit_form(editing, NOW)),
        (
            "Edit refused",
            reminders::form(reminders::Purpose::Edit(editing.id), values, &problems, NOW),
        ),
    ]
    .map(|(state, form)| Fixture {
        component: "Reminder edit page",
        state: state.to_owned(),
        document: reminders::edit_page(editing.id, &form),
        height: REMINDER_EDIT_HEIGHT,
    });
    tabs.into_iter().chain(edits).collect()
}

/// The Shopping tab with nothing to buy, with Items in every group and some bought lately, with
/// long names and notes, and with a refused form; an open Item row with a link in its note, one
/// being edited, one with its edit refused, and a bought one; Recently bought empty, folded and
/// unfolded; the Item form empty, refused, editing and with its edit refused; and the page an
/// Item is edited on without htmx.
fn shopping_pages() -> Vec<Fixture> {
    use shopping::fixtures::{bought, open, refused};
    use shopping::{Purpose, editing_row};

    let empty = shopping::form(
        Purpose::Add,
        ItemValues::default(),
        &ItemProblems::default(),
    );
    let (values, problems) = refused();
    let refused = shopping::form(Purpose::Add, values, &problems);
    let edited = &open()[2];
    let editing = shopping::edit_form(edited);
    let edit_refused = shopping::form(Purpose::Edit(edited.id), values, &problems);
    let tabs = shopping_tabs(&empty, &refused);
    let rows = [
        (
            "Open, with a link in its note",
            shopping::item_row(&open()[2]),
        ),
        ("Editing", editing_row(edited.id, &editing)),
        ("Edit refused", editing_row(edited.id, &edit_refused)),
        ("Bought", shopping::bought_row(&bought()[0])),
    ]
    .map(|(state, row)| Fixture {
        component: "Item row",
        state: state.to_owned(),
        document: document(
            "Item row",
            &html! { main.shell-main { ul.item-list { (row) } } },
        ),
        height: ITEM_ROW_HEIGHT,
    });
    let recently_bought = [
        ("Empty", shopping::recently_bought(&[], Fold::Unfolded)),
        ("Folded", shopping::recently_bought(&bought(), Fold::Folded)),
        (
            "Unfolded",
            shopping::recently_bought(&bought(), Fold::Unfolded),
        ),
    ]
    .map(|(state, section)| Fixture {
        component: "Recently bought",
        state: state.to_owned(),
        document: document("Recently bought", &html! { main.shell-main { (section) } }),
        height: RECENTLY_BOUGHT_HEIGHT,
    });
    let edit_page = Fixture {
        component: "Item edit page",
        state: "With errors".to_owned(),
        document: shopping::edit_page(&edit_refused),
        height: ITEM_FORM_HEIGHT,
    };
    let forms = [
        ("Empty", empty),
        ("With errors", refused),
        ("Editing", editing),
        ("Edit with errors", edit_refused),
    ]
    .map(|(state, form)| Fixture {
        component: "Item form",
        state: state.to_owned(),
        document: document("Item form", &html! { main.shell-main { (form) } }),
        height: ITEM_FORM_HEIGHT,
    });
    tabs.into_iter()
        .chain(rows)
        .chain(recently_bought)
        .chain(forms)
        .chain([edit_page])
        .collect()
}

/// The Shopping tab with nothing to buy; with every Urgency, the next brew day unplanned, planned
/// with no Batch and planned with one; with long names and notes; and with `refused` in place of
/// the `empty` form.
fn shopping_tabs(empty: &Markup, refused: &Markup) -> [Fixture; 6] {
    use shopping::fixtures::{bought, long, no_slot, open, slot_with_batch, slot_without_batch};

    [
        ("Nothing to buy", empty, Vec::new(), no_slot(), Vec::new()),
        ("Every Urgency", empty, open(), no_slot(), bought()),
        (
            "Next brewday, no Batch",
            empty,
            open(),
            slot_without_batch(),
            bought(),
        ),
        (
            "Next brewday with a Batch",
            empty,
            open(),
            slot_with_batch(),
            bought(),
        ),
        ("Long names and notes", empty, long(), no_slot(), Vec::new()),
        ("Refused", refused, open(), no_slot(), bought()),
    ]
    .map(|(state, form, open, next, bought)| Fixture {
        component: "Shopping tab",
        state: state.to_owned(),
        document: shopping::page(form, &open, &next, &bought),
        height: SHOPPING_HEIGHT,
    })
}

/// The Quotes tab empty, with a collection, and searched with and without matches; the random-Quote card with a Quote, a multi-line
/// one and none; a Quote row with and without Added by, a long one and a multi-line one; and the
/// add form empty and refused.
fn quotes_pages() -> Vec<Fixture> {
    use quotes::fixtures::{
        authors, collection, empty, found, imported, long, multiline, not_found, refused,
        remembered,
    };

    let authors = authors();
    let empty_form = quotes::form(
        QuotePurpose::Add,
        QuoteValues::default(),
        &QuoteProblems::default(),
        &authors,
    );
    let (values, problems) = refused();
    let refused_form = quotes::form(QuotePurpose::Add, values, &problems, &authors);

    let tabs = [
        Fixture {
            component: "Quotes tab",
            state: "No Quotes".to_owned(),
            document: quotes::page(None, &quotes::add_button(), &empty()),
            height: QUOTES_HEIGHT,
        },
        Fixture {
            component: "Quotes tab",
            state: "With a collection".to_owned(),
            document: quotes::page(Some(&multiline()), &quotes::add_button(), &collection()),
            height: QUOTES_COLLECTION_HEIGHT,
        },
        Fixture {
            component: "Quotes tab",
            state: "Search results".to_owned(),
            document: quotes::page(Some(&remembered()), &quotes::add_button(), &found()),
            height: QUOTES_COLLECTION_HEIGHT,
        },
        Fixture {
            component: "Quotes tab",
            state: "Search with no matches".to_owned(),
            document: quotes::page(Some(&remembered()), &quotes::add_button(), &not_found()),
            height: QUOTES_HEIGHT,
        },
    ];
    let cards = [
        ("A Quote", Some(remembered())),
        ("Multi-line Quote", Some(multiline())),
        ("Empty", None),
    ]
    .map(|(state, quote)| Fixture {
        component: "Random-Quote card",
        state: state.to_owned(),
        document: document(
            "Random-Quote card",
            &html! { main.shell-main { (quotes::random_card(quote.as_ref(), false)) } },
        ),
        height: RANDOM_CARD_HEIGHT,
    });
    let rows = [
        ("With Added by", quotes::quote_row(&remembered()), 200),
        ("Without Added by", quotes::quote_row(&imported()), 200),
        ("Long text", quotes::quote_row(&long()), 420),
        (
            "Multi-line text",
            quotes::quote_row(&multiline()),
            QUOTE_ROW_HEIGHT,
        ),
    ]
    .map(|(state, row, height)| Fixture {
        component: "Quote row",
        state: state.to_owned(),
        document: document(
            "Quote row",
            &html! { main.shell-main { ul.quote-list { (row) } } },
        ),
        height,
    });
    let forms =
        [("Empty", empty_form), ("With errors", refused_form)].map(|(state, form)| Fixture {
            component: "Quote form",
            state: state.to_owned(),
            document: document("Quote form", &html! { main.shell-main { (form) } }),
            height: QUOTE_FORM_HEIGHT,
        });
    tabs.into_iter()
        .chain(cards)
        .chain(rows)
        .chain(forms)
        .chain(quote_edits())
        .collect()
}

/// The Batches tab empty, with a log, on a later page and with a refused form; a Batch row in
/// each Stage and each Stage's badge; a Batch's page in each Stage, and once it's gone; the add
/// form empty and refused; the edit page filled in and refused; and the Brew day and Bottling
/// forms empty, filled in and refused, in place and on their own page.
fn batches_pages() -> Vec<Fixture> {
    use batches::fixtures::{
        empty, later_page, log, long, pilsner, planned_pilsner, refused, refused_edit,
    };

    let empty_form = batches::form(
        BatchPurpose::Add,
        BatchValues::default(),
        &BatchProblems::default(),
    );
    let (values, problems) = refused();
    let refused_form = batches::form(BatchPurpose::Add, values, &problems);
    let no_slots = slots::slots_section(&[], &slots::plan_form("", None, slots::fixtures::TODAY));
    let with_slots = slots::slots_section(
        &slots::fixtures::several(),
        &slots::plan_form("", None, slots::fixtures::TODAY),
    );
    let tabs = [
        ("No Batches", &no_slots, &empty_form, empty()),
        ("With a log", &no_slots, &empty_form, log()),
        ("With Brewing slots", &with_slots, &empty_form, log()),
        ("Later page", &no_slots, &empty_form, later_page()),
        ("Form with errors", &no_slots, &refused_form, log()),
    ]
    .map(|(state, slots, form, log)| Fixture {
        component: "Batches tab",
        state: state.to_owned(),
        document: batches::page(slots, form, &log),
        height: BATCHES_HEIGHT,
    });
    let rows = [
        ("Planned, waiting for a slot", planned_pilsner()),
        ("Planned, with a slot", batches::fixtures::slotted_pilsner()),
        ("In the fermenter, long name", long()),
        ("Bottled", pilsner()),
    ]
    .map(|(state, batch)| Fixture {
        component: "Batch row",
        state: state.to_owned(),
        document: document(
            "Batch row",
            &html! { main.shell-main { ul.batch-list { (batches::batch_row(&batch)) } } },
        ),
        height: 160,
    });
    let badges = Fixture {
        component: "Stage badge",
        state: "Planned, in the fermenter and Bottled".to_owned(),
        document: document(
            "Stage badge",
            &html! {
                main.shell-main {
                    @for stage in [Stage::Planned, Stage::Brewed, Stage::Bottled] {
                        p { (batches::stage_badge(stage)) }
                    }
                }
            },
        ),
        height: 220,
    };
    let forms =
        [("Empty", empty_form), ("With errors", refused_form)].map(|(state, form)| Fixture {
            component: "Batch form",
            state: state.to_owned(),
            document: document("Batch form", &html! { main.shell-main { (form) } }),
            height: BATCH_FORM_HEIGHT,
        });
    let number = pilsner().number;
    let (values, problems) = refused_edit();
    let edit_pages = [
        ("Filled in", batches::edit_form(&pilsner())),
        (
            "Edit refused",
            batches::form(BatchPurpose::Edit(number), values, &problems),
        ),
    ]
    .map(|(state, form)| Fixture {
        component: "Batch edit page",
        state: state.to_owned(),
        document: batches::edit_page(number, &form),
        height: BATCH_FORM_HEIGHT,
    });
    tabs.into_iter()
        .chain(rows)
        .chain([badges])
        .chain(batch_pages())
        .chain(forms)
        .chain(edit_pages)
        .chain(numbers_forms())
        .chain(slot_pages())
        .collect()
}

/// The Brewing slots section with no slot, one, several and one in each state, and with a
/// refused plan; the slot's page with nothing chosen, with a Batch, without Planned Batches,
/// asking "Did you brew?", with a refused link and with a refused move; and the page once a slot
/// is gone.
fn slot_pages() -> Vec<Fixture> {
    use slots::fixtures::{
        LINK_REFUSED, REFUSED, TODAY, asking, every_state, linked, one, planned, several,
    };

    let empty_plan = slots::plan_form("", None, TODAY);
    let (value, message) = REFUSED;
    let refused_plan = slots::plan_form(value, Some(message), TODAY);
    let sections = [
        ("No slot", Vec::new(), &empty_plan),
        ("One slot", one(), &empty_plan),
        ("Several slots", several(), &empty_plan),
        ("Every state", every_state(), &empty_plan),
        ("Plan refused", one(), &refused_plan),
    ]
    .map(|(state, list, plan)| Fixture {
        component: "Brewing slots",
        state: state.to_owned(),
        document: document(
            "Brewing slots",
            &html! { main.shell-main { (slots::slots_section(&list, plan)) } },
        ),
        height: SLOTS_HEIGHT,
    });
    let free = one().remove(0);
    let planned = planned();
    let refused_move = slots::move_form(
        free.id,
        "2026-11-10",
        Some("Pick today or a later date."),
        TODAY,
    );
    let moves = [
        ("Nothing chosen", free.clone(), &planned[..], None, None),
        ("With a Batch", linked(), &planned[..], None, None),
        ("No Planned Batch", free.clone(), &[][..], None, None),
        ("Did you brew?", asking(), &planned[..], None, None),
        (
            "Link refused",
            free.clone(),
            &planned[..],
            Some(LINK_REFUSED),
            None,
        ),
        (
            "Move refused",
            free,
            &planned[..],
            None,
            Some(&refused_move),
        ),
    ]
    .map(|(state, slot, planned, link_problem, move_form)| {
        let filled_in = slots::move_form(slot.id, &slot.date.to_string(), None, TODAY);
        Fixture {
            component: "Brewing slot page",
            state: state.to_owned(),
            document: slots::slot_page(
                &slot,
                planned,
                link_problem,
                move_form.unwrap_or(&filled_in),
            ),
            height: PAGE_HEIGHT,
        }
    });
    let gone = Fixture {
        component: "Brewing slot page",
        state: "Slot gone".to_owned(),
        document: slots::gone_page(),
        height: PAGE_HEIGHT,
    };
    sections.into_iter().chain(moves).chain([gone]).collect()
}

/// A Batch's page in each Stage, without Notes, and once it's gone; with a Recipe, before and
/// after brewing and when it can't be read; the Recipe viewer for a full and a sparse Recipe;
/// and a refused Recipe upload.
fn batch_pages() -> Vec<Fixture> {
    use crate::domain::batches::AttachedRecipe;
    use crate::domain::batches::recipe_fixtures::{attached, full, sparse};
    use batches::fixtures::{bare, fermenting_pilsner, pilsner, planned_pilsner};

    let with_recipe = [
        (
            "Bottled, with a Recipe",
            batches::batch_page(
                &pilsner(),
                Some(&attached()),
                &batches::SlotChoice::default(),
            ),
            RECIPE_PAGE_HEIGHT,
        ),
        (
            "Planned, with Recipe targets",
            batches::batch_page(&bare(), Some(&attached()), &batches::SlotChoice::default()),
            RECIPE_PAGE_HEIGHT,
        ),
        (
            "Recipe unreadable",
            batches::batch_page(
                &bare(),
                Some(&AttachedRecipe::Unreadable),
                &batches::SlotChoice::default(),
            ),
            BATCH_PAGE_HEIGHT,
        ),
    ]
    .map(|(state, document, height)| Fixture {
        component: "Batch page",
        state: state.to_owned(),
        document,
        height,
    });
    let viewers = [
        ("Full", full(), RECIPE_VIEWER_HEIGHT),
        ("Sparse", sparse(), 200),
    ]
    .map(|(state, recipe, height)| Fixture {
        component: "Recipe viewer",
        state: state.to_owned(),
        document: document(
            "Recipe viewer",
            &html! { main.shell-main { (batches::recipe_viewer(&recipe)) } },
        ),
        height,
    });
    let upload = Fixture {
        component: "Recipe upload",
        state: "Refused".to_owned(),
        document: document(
            "Recipe upload",
            &html! {
                main.shell-main {
                    (batches::recipe_upload(
                        pilsner().number,
                        true,
                        Some("This BeerXML file has no recipe in it, only ingredients or other records. Export the recipe itself and try again."),
                    ))
                }
            },
        ),
        height: 400,
    };
    let stages = [
        (
            "Planned, with Notes, edited",
            batches::batch_page(&planned_pilsner(), None, &batches::SlotChoice::default()),
        ),
        (
            "In the fermenter, FG read early",
            batches::batch_page(&fermenting_pilsner(), None, &batches::SlotChoice::default()),
        ),
        (
            "Bottled, with ABV",
            batches::batch_page(&pilsner(), None, &batches::SlotChoice::default()),
        ),
        (
            "Without Notes",
            batches::batch_page(&bare(), None, &batches::SlotChoice::default()),
        ),
        ("Gone", batches::gone_page(bare().number)),
    ]
    .map(|(state, document)| Fixture {
        component: "Batch page",
        state: state.to_owned(),
        document,
        height: BATCH_PAGE_HEIGHT,
    });
    stages
        .into_iter()
        .chain(slotted_batch_pages())
        .chain(with_recipe)
        .chain(viewers)
        .chain([upload])
        .collect()
}

/// A Planned Batch's page waiting with free slots to pick, on a slot with others free to move
/// to, and with a refused link.
fn slotted_batch_pages() -> [Fixture; 3] {
    use batches::fixtures::{planned_pilsner, slotted_pilsner};

    let free = slots::fixtures::free();
    let choice = batches::SlotChoice {
        free: &free,
        problem: None,
    };
    let refused = batches::SlotChoice {
        free: &free,
        problem: Some("That slot is gone: someone cancelled it. Pick another."),
    };
    [
        (
            "Planned, waiting with free slots",
            planned_pilsner(),
            choice,
        ),
        ("Planned, with a slot", slotted_pilsner(), choice),
        ("Planned, slot link refused", planned_pilsner(), refused),
    ]
    .map(|(state, batch, slots)| Fixture {
        component: "Batch page",
        state: state.to_owned(),
        document: batches::batch_page(&batch, None, &slots),
        height: BATCH_PAGE_HEIGHT,
    })
}

/// A Batch's Brew day and Bottling forms empty, filled in and refused, as htmx swaps them into
/// the Batch's page; and one on its own page, as it is without htmx.
fn numbers_forms() -> Vec<Fixture> {
    use batches::fixtures::{bare, pilsner, refused_bottling, refused_brew_day};
    use batches::{BottlingProblems, BottlingValues, BrewDayProblems, BrewDayValues};

    let today = jiff::civil::date(2026, 11, 10);
    let (brew_day, brew_day_problems) = refused_brew_day();
    let (bottling, bottling_problems) = refused_bottling();
    let number = pilsner().number;
    let forms = [
        (
            "Brew day form",
            "Empty, today",
            batches::brew_day_form(
                bare().number,
                &BrewDayValues::of(&bare(), today),
                &BrewDayProblems::default(),
            ),
        ),
        (
            "Brew day form",
            "With errors",
            batches::brew_day_form(number, &brew_day, &brew_day_problems),
        ),
        (
            "Bottling form",
            "Filled in",
            batches::bottling_form(
                number,
                &BottlingValues::of(&pilsner(), today),
                &BottlingProblems::default(),
            ),
        ),
        (
            "Bottling form",
            "With errors",
            batches::bottling_form(number, &bottling, &bottling_problems),
        ),
    ]
    .map(|(component, state, form)| Fixture {
        component,
        state: state.to_owned(),
        document: document(component, &html! { main.shell-main { (form) } }),
        height: BATCH_FORM_HEIGHT,
    });
    let page = Fixture {
        component: "Bottling page",
        state: "Without htmx, with errors".to_owned(),
        document: batches::numbers_page(
            number,
            &batches::bottling_form(number, &bottling, &bottling_problems),
        ),
        height: BATCH_FORM_HEIGHT,
    };
    forms.into_iter().chain([page]).collect()
}

/// A Quote's row while it's edited, filled in and refused, and once it's gone; and the page it's
/// edited on without htmx.
fn quote_edits() -> Vec<Fixture> {
    use quotes::fixtures::{authors, multiline, refused_edit, remembered};

    let authors = authors();
    let editing = remembered();
    let editing_form = quotes::editing_row(editing.id, &quotes::edit_form(&editing, &authors));
    let (values, problems) = refused_edit();
    let refused_edit_form = quotes::editing_row(
        editing.id,
        &quotes::form(QuotePurpose::Edit(editing.id), values, &problems, &authors),
    );
    let edits = [
        ("Editing, filled in", editing_form),
        ("Editing, with errors", refused_edit_form),
        ("Gone", quotes::gone_row(editing.id)),
    ]
    .map(|(state, row)| Fixture {
        component: "Quote form",
        state: state.to_owned(),
        document: document(
            "Quote form",
            &html! { main.shell-main { ul.quote-list { (row) } } },
        ),
        height: QUOTE_FORM_HEIGHT,
    });
    let edit_page = Fixture {
        component: "Quote edit page",
        state: "Without htmx".to_owned(),
        document: quotes::edit_page(&quotes::edit_form(&multiline(), &authors)),
        height: QUOTES_HEIGHT,
    };
    edits.into_iter().chain([edit_page]).collect()
}

/// The page an entry form in `purpose` sits on, its delete button posting to `delete`.
const fn entry_page(purpose: Purpose, delete: &str) -> EntryPage<'_> {
    match purpose {
        Purpose::Log => EntryPage::New,
        Purpose::Edit => EntryPage::Edit { delete },
    }
}

/// History unfiltered, filtered by Member and month, with nothing logged, with nothing to list,
/// and with a month that isn't one.
fn history_pages() -> [Fixture; 5] {
    use finance::fixtures::{JOSEF, ONDRA, SIMON, crew, member};
    use history::fixtures::{empty, filtered, october, ondra_in_october, unfiltered};

    let nobody = HistoryFilter {
        member: Some(SIMON),
        month: Some(october()),
    };
    let mistyped = Filters {
        applied: HistoryFilter::default(),
        month: "Oct",
        month_problem: Some(NOT_A_MONTH),
    };
    [
        (
            "Unfiltered",
            ONDRA,
            Filters::of(HistoryFilter::default(), ""),
            unfiltered(),
            HISTORY_HEIGHT,
        ),
        (
            "Filtered",
            ONDRA,
            Filters::of(ondra_in_october(), "2026-10"),
            filtered(),
            FILTERED_HISTORY_HEIGHT,
        ),
        (
            "Nothing logged",
            JOSEF,
            Filters::of(HistoryFilter::default(), ""),
            empty(),
            PAGE_HEIGHT,
        ),
        (
            "Nothing matches",
            SIMON,
            Filters::of(nobody, "2026-10"),
            empty(),
            PAGE_HEIGHT,
        ),
        (
            "Mistyped month",
            JOSEF,
            mistyped,
            unfiltered(),
            HISTORY_HEIGHT,
        ),
    ]
    .map(|(state, me, filters, history, height)| Fixture {
        component: "History",
        state: state.to_owned(),
        document: history::page(&member(me), &crew(), &filters, &history),
        height,
    })
}

/// The Payment form empty, pre-filled from a Settlement, with problems, and editing a Payment.
fn payment_forms() -> [Fixture; 4] {
    use finance::fixtures::{ONDRA, SIMON, crew, prefilled_payment};

    let mistaken_payment = PaymentValues {
        amount: "0",
        description: "",
        sender: SIMON,
        recipient: Some(SIMON),
    };
    let payment_problems = PaymentProblems {
        amount: Some("Enter an amount from 1 to 1 000 000 Kč.".to_owned()),
        description: None,
        sender: None,
        recipient: Some("Choose someone other than the Sender.".to_owned()),
    };
    [
        (
            "Empty",
            Purpose::Log,
            PaymentValues::blank(ONDRA),
            PaymentProblems::default(),
        ),
        (
            "Pre-filled",
            Purpose::Log,
            prefilled_payment(),
            PaymentProblems::default(),
        ),
        (
            "With errors",
            Purpose::Log,
            mistaken_payment,
            payment_problems,
        ),
        (
            "Editing",
            Purpose::Edit,
            prefilled_payment(),
            PaymentProblems::default(),
        ),
    ]
    .map(|(state, purpose, values, problems)| Fixture {
        component: "Payment form",
        state: state.to_owned(),
        document: finance::payment_page(
            entry_page(purpose, "/payments/4/delete"),
            &finance::payment_form("/payments/4", purpose, &crew(), &values, &problems),
        ),
        height: PAYMENT_FORM_HEIGHT,
    })
}

/// The pages a signed-in Member changes their display name on and turns notifications on
/// from, the name form and the notifications control.
fn signed_in_pages() -> Vec<Fixture> {
    let welcome = [
        ("Just joined", PushState::Off),
        ("Notifications on", PushState::On),
    ]
    .map(|(state, push)| Fixture {
        component: "Welcome page",
        state: state.to_owned(),
        document: welcome::page(
            CREW_NAME,
            &display_name::form("/welcome/name", "Ondra", None),
            &push::control(push, &PUSH_URLS),
            "/welcome/done",
        ),
        height: WELCOME_HEIGHT,
    });
    let settings = [
        (
            "One passkey",
            passkeys::fixtures::one(),
            false,
            PushState::Off,
            SETTINGS_HEIGHT,
        ),
        (
            "Several passkeys",
            passkeys::fixtures::several(),
            false,
            PushState::Off,
            SEVERAL_PASSKEYS_HEIGHT,
        ),
        (
            "The Admin's",
            passkeys::fixtures::one(),
            true,
            PushState::Off,
            ADMIN_SETTINGS_HEIGHT,
        ),
        (
            "Notifications on",
            passkeys::fixtures::one(),
            false,
            PushState::On,
            SETTINGS_HEIGHT,
        ),
    ]
    .map(|(state, passkeys, admin, push, height)| Fixture {
        component: "Settings page",
        state: state.to_owned(),
        document: settings::page(
            &display_name::form("/settings/name", "Ondra", None),
            &passkeys::list(PASSKEYS_BASE, &passkeys, None),
            &CeremonyUrls {
                start: format!("{PASSKEYS_BASE}/start"),
                finish: format!("{PASSKEYS_BASE}/finish"),
            },
            &push::control(push, &PUSH_URLS),
            admin.then_some(ADMIN_HREF),
            "/log-out",
        ),
        height,
    });
    let push_controls = PushState::ALL.map(|state| Fixture {
        component: "Notifications control",
        state: push_state_name(state).to_owned(),
        document: document(
            "Notifications control",
            &html! {
                main.standalone {
                    section.section.narrow aria-labelledby="push" {
                        h2 #push { "Notifications" }
                        (push::control(state, &PUSH_URLS))
                    }
                }
            },
        ),
        height: PUSH_CONTROL_HEIGHT,
    });
    let refused = "Someone in the Crew is already called Josef.";
    let name_forms = [
        ("Unchanged", "Ondra", None),
        ("Saved", "Ondřej", Some(Outcome::Saved)),
        ("Refused", "Josef", Some(Outcome::Refused(refused))),
    ]
    .map(|(state, value, outcome)| Fixture {
        component: "Display name form",
        state: state.to_owned(),
        document: document(
            "Display name form",
            &html! { main.standalone { (display_name::form("/welcome/name", value, outcome)) } },
        ),
        height: 260,
    });
    welcome
        .into_iter()
        .chain(settings)
        .chain(push_controls)
        .chain(name_forms)
        .collect()
}

const fn push_state_name(state: PushState) -> &'static str {
    match state {
        PushState::Off => "Off",
        PushState::On => "On",
        PushState::InstallFirst => "iPhone outside the installed app",
        PushState::Blocked => "Blocked",
    }
}

/// The passkey list after each thing a Member can do to it.
fn passkey_lists() -> Vec<Fixture> {
    let passkeys = passkeys::fixtures::several();
    let lost = PasskeyName::parse("Lost phone").expect("fixture names are valid");
    let refused = "Enter a name for the passkey.";
    [
        ("Renamed", Some(PasskeyOutcome::Renamed(passkeys[1].id))),
        (
            "Rename refused",
            Some(PasskeyOutcome::RenameRefused {
                id: passkeys[1].id,
                value: "",
                message: refused,
            }),
        ),
        ("Deleted", Some(PasskeyOutcome::Deleted(&lost))),
    ]
    .map(|(state, outcome)| Fixture {
        component: "Passkey list",
        state: state.to_owned(),
        document: document(
            "Passkey list",
            &html! {
                main.standalone { (passkeys::list(PASSKEYS_BASE, &passkeys, outcome)) }
            },
        ),
        height: 640,
    })
    .into()
}

/// Each logo on every background it was drawn for, whatever scheme the viewer's browser is in,
/// and the app icon on the manifest's background colour, as Android's splash screen shows it.
fn brand() -> Markup {
    let hero = |asset: assets::Asset, background: &str| {
        html! {
            div style={ "padding: 1rem; background: " (background) } {
                img src=(asset.url) alt="" width="180" height="180" style="display: block";
            }
        }
    };
    document(
        "Brand",
        &html! {
            @for band in ["#3E5B2B", "#263820"] {
                header.shell-header style={ "background: " (band) } { (header_logo()) }
            }
            div style="display: flex; flex-wrap: wrap" {
                (hero(assets::LOGO_HERO_LIGHT, "#EEF0E4"))
                (hero(assets::LOGO_HERO_DARK, "#15180F"))
                (hero(assets::ICON_512, "#3E5B2B"))
            }
        },
    )
}

#[must_use]
pub fn page(fixtures: &[Fixture]) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Gallery | Asgard" }
                style { (GALLERY_CSS) }
            }
            body {
                h1 { "Component gallery" }
                @for (index, fixture) in fixtures.iter().enumerate() {
                    @if index == 0 || fixtures[index - 1].component != fixture.component {
                        h2 { (fixture.component) }
                    }
                    section {
                        h3 { (fixture.state) }
                        div.frames {
                            figure {
                                figcaption { "Phone, " (PHONE_WIDTH) "px" }
                                iframe title={ (fixture.component) ", " (fixture.state) ", phone" }
                                    sandbox="allow-same-origin"
                                    width=(PHONE_WIDTH) height=(fixture.height)
                                    srcdoc=(fixture.document.0) {}
                            }
                            figure {
                                figcaption { "Desktop, " (DESKTOP_WIDTH) "px, shown at half size" }
                                div.desktop style={ "height: " (fixture.height / 2) "px" } {
                                    iframe title={ (fixture.component) ", " (fixture.state) ", desktop" }
                                        sandbox="allow-same-origin"
                                        width=(DESKTOP_WIDTH) height=(fixture.height)
                                        srcdoc=(fixture.document.0) {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The gallery's own chrome. Debug-only, so it stays out of the app's stylesheet.
const GALLERY_CSS: &str = "
body { margin: 0; padding: 1.5rem; font-family: system-ui, sans-serif; background: #e4e6da; color: #22261b; }
h2 { margin-top: 2.5rem; }
.frames { display: flex; flex-wrap: wrap; gap: 1.5rem; align-items: flex-start; }
figure { margin: 0; }
figcaption { font-size: 0.875rem; color: #5e6450; margin-bottom: 0.5rem; }
iframe { display: block; border: 1px solid #b9bea6; background: #fff; }
.desktop { width: 640px; overflow: hidden; }
.desktop iframe { transform: scale(0.5); transform-origin: top left; }
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_every_component_in_every_state() {
        let html = page(&fixtures()).into_string();

        for component in [
            "Bottom navigation",
            "App shell",
            "Brand",
            "Offline page",
            "Invite page",
            "Device link page",
            "Link error page",
            "Sign-in page",
            "Welcome page",
            "Settings page",
            "Display name form",
            "Notifications control",
            "Passkey list",
            "Admin page",
            "Finance home",
            "Expense form",
            "Payment form",
            "Entry row",
            "History",
            "Reminders tab",
            "Reminder edit page",
            "Shopping tab",
            "Item row",
            "Recently bought",
            "Item form",
        ] {
            assert!(
                html.contains(&format!("<h2>{component}</h2>")),
                "{component}"
            );
        }
        for tab in Tab::ALL {
            assert!(html.contains(&format!("<h3>{} active</h3>", tab.label())));
            assert!(html.contains(&format!("<h3>{} tab</h3>", tab.label())));
        }
        for state in [
            "Logos on every background",
            "No network",
            "Valid link",
            "Already signed in",
            "Naming the Admin",
            "Before there is an Admin",
            "Signed out",
            "Just joined",
            "One passkey",
            "Several passkeys",
            "Renamed",
            "Rename refused",
            "Deleted",
            "Unchanged",
            "Saved",
            "Refused",
            "The Admin's",
            "No open links",
            "Open links",
            "Link just made",
            "Device link just made",
            "Alone in the Crew",
            "Name refused",
            "Link revoked",
            "Owes",
            "Owed",
            "Even",
            "Nothing logged",
            "Empty",
            "With errors",
            "Pre-filled",
            "Expense",
            "Payment",
            "Payment without a description",
            "Expense its Payer may change",
            "Edited",
            "Edited Payment",
            "Editing",
            "Nothing pending",
            "Pending",
            "Form with errors",
            "Opened",
            "Edit refused",
            "Nothing to buy",
            "Every Urgency",
            "Long names and notes",
            "Open, with a link in its note",
            "Bought",
            "Folded",
            "Unfolded",
            "Unfiltered",
            "Filtered",
            "Nothing matches",
            "Mistyped month",
            "Notifications on",
            "Off",
            "On",
            "iPhone outside the installed app",
            "Blocked",
        ] {
            assert!(html.contains(&format!("<h3>{state}</h3>")), "{state}");
        }
    }

    #[test]
    fn lists_every_batches_component_in_every_state() {
        let html = page(&fixtures()).into_string();

        for component in [
            "Batches tab",
            "Batch row",
            "Stage badge",
            "Batch page",
            "Batch form",
            "Batch edit page",
            "Brew day form",
            "Bottling form",
            "Bottling page",
            "Recipe viewer",
            "Recipe upload",
            "Brewing slots",
            "Brewing slot page",
        ] {
            assert!(
                html.contains(&format!("<h2>{component}</h2>")),
                "{component}"
            );
        }
        for state in [
            "No Batches",
            "With a log",
            "Later page",
            "Form with errors",
            "Planned, waiting for a slot",
            "Planned, with a slot",
            "Planned, waiting with free slots",
            "Planned, slot link refused",
            "No slot",
            "Every state",
            "Plan refused",
            "Nothing chosen",
            "With a Batch",
            "No Planned Batch",
            "Did you brew?",
            "Link refused",
            "Move refused",
            "Slot gone",
            "In the fermenter, long name",
            "Bottled",
            "Planned, with Notes, edited",
            "In the fermenter, FG read early",
            "Bottled, with ABV",
            "Without Notes",
            "Empty, today",
            "With errors",
            "Without htmx, with errors",
            "Gone",
            "Filled in",
            "Edit refused",
            "Bottled, with a Recipe",
            "Planned, with Recipe targets",
            "Recipe unreadable",
            "Full",
            "Sparse",
            "Refused",
        ] {
            assert!(html.contains(&format!("<h3>{state}</h3>")), "{state}");
        }
    }

    #[test]
    fn shows_each_fixture_at_phone_and_desktop_width() {
        let fixtures = fixtures();
        let html = page(&fixtures).into_string();

        assert_eq!(
            html.matches(&format!("width=\"{PHONE_WIDTH}\"")).count(),
            fixtures.len()
        );
        assert_eq!(
            html.matches(&format!("width=\"{DESKTOP_WIDTH}\"")).count(),
            fixtures.len()
        );
    }
}
