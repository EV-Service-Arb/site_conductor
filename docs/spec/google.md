# Google

r[google.keys-on-accounts]

A Google key or credential that bills on us is used only as a slot of an account in
`nix/google.nix`; every account there has its billing export, read by the conductor and held
by the guard. A gmail's own (its Maps key, its Business Profile client) bills on that gmail's
project.

r[google.guarded]

Every API enabled on a project billed to an account in `nix/google.nix` is either held by a lever
the guard sets before spending happens, or named in that account's `unguarded`. Anything else is
disabled by the guard. Each account names every slice's real money a day and a month (`allow`);
one of all zeros spends only free units and credits.

r[google.buy-as-user]

A Places request another service asks the conductor to buy is bought for the person whose User
assertion, signed by the conductor for that service and alive, it carries, as their own request
would be: their GBP's gmail's key, their other gmails', then the rotation at the price, refused below
it. It is bought only if it is one of the requests sa_places makes of Places, at the SKU its field
mask says.
