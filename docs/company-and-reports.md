# Company setup, sign-in, settings, and reports

Cash Ledger keeps company configuration, local credentials, and the cash ledger in the
same SQLite file. Existing installations retain their registers and movements;
company setup adds its own tables and does not change historical cash records.

## First use and sign-in

Complete company setup once, including the company name and an administrator
username and password. Passwords require at least 12 characters. Credentials
are stored as salted Argon2id password hashes, rather than plaintext.
Later launches require sign-in before displaying the workspace. Sign out when
leaving the register unattended.

Setup and sign-in forms scroll vertically when they exceed the window height.
Use the mouse wheel, trackpad, or vertical scrollbar to reach the bottom controls.

There is one local administrator account. This is application access control,
not database encryption: protect the Windows account and database backups.
There is no cloud identity service or email password recovery. Preserve your
administrator credentials; do not delete the ledger to reset a password.

## Company settings

Company settings include the name, address, written cash-handling rules, closing
variance warning tolerance, printer name, and A4 or Letter paper size. Written
rules are guidance for staff. The numerical tolerance warns during closing
review; the operator can still explicitly confirm the reconciled count.
Amounts and registers continue to use CAD with two decimal places.

## Live dashboard and backups

The Dashboard shows CAD activity across registers for the last 7, 30, 90, or 365
inclusive business dates. It includes incoming and outgoing totals, net movement,
movement counts, open days, daily net-flow and register charts, and the twenty
largest closing count differences. Opening cash is not revenue; net movement is
cash received minus cash paid out. Closing differences are signed and can offset
each other in the summary.

Analytics use an independent read-only connection and a consistent snapshot.
Only aggregate chart points and bounded detail lists return to the UI. Refresh
runs in the background every fifteen seconds after the previous read completes
while the dashboard is active and signed in. Query timing describes that read,
not a guarantee for all database sizes. Queries have a ten-second execution
deadline and a two-second lock timeout. Errors preserve the previous snapshot.

Save verified backup saves a consistent online SQLite snapshot, including committed
records in the WAL. Choose a new filename: existing destinations and SQLite
sidecars are rejected. A temporary snapshot is checked for integrity before
publication. Backups contain company credentials and financial data; store them
with appropriate access restrictions. Backup scheduling, encryption, and an
in-app restore workflow are not implemented.

The isolated synthetic workload example is described in [Performance testing](performance.md).

## Reports and printing

Reports select an inclusive range of business dates and optionally a register.
They include session opening cash, cash in, cash out, expected cash, physical
closing count and difference, plus movement details from both directions.
Open sessions have no closing count or difference yet. Report queries read a
consistent SQLite snapshot. Requests exceeding 10,000 sessions or 100,000
movements must use a narrower range, rather than silently truncating results.

CSV exports label financial columns in integer currency minor units (100 means
CAD 1.00). Text fields are quoted and escaped, and potentially executable
spreadsheet formulas are prefixed with an apostrophe. Treat exported files as
company financial data. Exports require a `.csv` filename, reject database and
SQLite destinations, and replace the selected CSV through a temporary file.
The on-screen preview shows up to 100 sessions; CSV and printed reports include
the complete matching result within the report size limits.

Native printing is supported on Windows. Leave the printer name blank to use
the Windows default, or supply the installed printer's exact name. Configure
the paper size in Settings. Reports render through the Windows printer driver;
they do not send raw text to a printer or depend on an external document viewer.
The driver may display additional UI, including a destination prompt for a PDF
printer. A successful print submission does not prove paper was produced;
check the Windows print queue for driver or device failures.

Printing and database work run on the background executor. Other platforms can
export CSV but do not support this Windows printing implementation.

## Implementation references

- [RustCrypto Argon2](https://docs.rs/argon2/0.5.3/argon2/) documents PHC password
  hashing and verification with random salts.
- [Windows GDI print lifecycle](https://learn.microsoft.com/en-us/windows/win32/printdocs/gdi-print-api-functions)
  documents document/page start, completion, and abort operations.
- [StartDocW](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-startdocw)
  starts a driver-rendered print job.
- [windows-sys printing bindings](https://docs.rs/windows-sys/0.61.2/windows_sys/Win32/Graphics/Printing/index.html)
  expose the Windows printer APIs to Rust.
