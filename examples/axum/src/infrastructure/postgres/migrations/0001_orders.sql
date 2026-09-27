CREATE TABLE reservations (
    id uuid PRIMARY KEY,
    sku text NOT NULL CHECK (length(sku) BETWEEN 1 AND 64),
    quantity integer NOT NULL CHECK (quantity BETWEEN 1 AND 1000),
    unit_price_yen bigint NOT NULL CHECK (unit_price_yen BETWEEN 1 AND 1000000000),
    expires_at timestamptz NOT NULL,
    confirmed boolean NOT NULL DEFAULT false
);
CREATE INDEX reservations_expiry ON reservations (expires_at, id) WHERE NOT confirmed;
CREATE TABLE orders (
    id uuid PRIMARY KEY,
    reservation_id uuid NOT NULL UNIQUE REFERENCES reservations (id),
    sku text NOT NULL,
    quantity integer NOT NULL CHECK (quantity > 0),
    total_yen bigint NOT NULL CHECK (total_yen > 0),
    confirmed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX orders_confirmed_at ON orders (confirmed_at) INCLUDE (total_yen);
CREATE TABLE daily_sales (
    day date PRIMARY KEY,
    order_count bigint NOT NULL CHECK (order_count >= 0),
    total_yen bigint NOT NULL CHECK (total_yen >= 0),
    rebuilt_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
