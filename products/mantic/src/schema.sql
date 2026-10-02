CREATE TABLE config (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE CHECK (length(trim(name)) > 0)
);
CREATE TABLE config_item (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    config_id INTEGER NOT NULL REFERENCES config(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    amount_cents INTEGER NOT NULL CHECK (amount_cents > 0),
    first_due TEXT NOT NULL,
    repeat_unit TEXT CHECK (repeat_unit IN ('day', 'week', 'month', 'year')),
    repeat_every INTEGER,
    end_date TEXT,
    CHECK ((repeat_unit IS NULL AND repeat_every IS NULL) OR
           (repeat_unit IS NOT NULL AND repeat_every IS NOT NULL AND repeat_every BETWEEN 1 AND 4294967295)),
    CHECK (end_date IS NULL OR end_date >= first_due)
);
CREATE INDEX config_item_owner ON config_item(config_id);
