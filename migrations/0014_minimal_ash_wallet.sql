CREATE TABLE ash_wallets (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    balance INTEGER NOT NULL DEFAULT 0,
    wallet_version BIGINT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id),
    FOREIGN KEY (namespace_id, account_id)
        REFERENCES accounts(namespace_id, account_id) ON DELETE CASCADE,
    CONSTRAINT ash_wallet_account_id_exact CHECK (octet_length(account_id) = 16),
    CONSTRAINT ash_wallet_balance_bounded CHECK (balance BETWEEN 0 AND 99999),
    CONSTRAINT ash_wallet_version_positive CHECK (wallet_version > 0)
);

CREATE TABLE ash_mutation_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    payload_hash BYTEA NOT NULL,
    mutation_kind SMALLINT NOT NULL,
    reason_code TEXT NOT NULL,
    source_id TEXT NOT NULL,
    content_version TEXT NOT NULL,
    requested_amount INTEGER NOT NULL,
    result_code SMALLINT NOT NULL,
    before_balance INTEGER NOT NULL,
    after_balance INTEGER NOT NULL,
    pre_wallet_version BIGINT NOT NULL,
    post_wallet_version BIGINT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id)
        REFERENCES ash_wallets(namespace_id, account_id) ON DELETE CASCADE,
    CONSTRAINT ash_result_account_id_exact CHECK (octet_length(account_id) = 16),
    CONSTRAINT ash_result_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT ash_result_payload_hash_exact CHECK (
        octet_length(payload_hash) = 32
        AND payload_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT ash_result_kind_known CHECK (mutation_kind BETWEEN 0 AND 1),
    CONSTRAINT ash_result_reason_bounded CHECK (char_length(reason_code) BETWEEN 1 AND 64),
    CONSTRAINT ash_result_source_bounded CHECK (char_length(source_id) BETWEEN 1 AND 128),
    CONSTRAINT ash_result_content_bounded CHECK (char_length(content_version) BETWEEN 1 AND 128),
    CONSTRAINT ash_result_amount_bounded CHECK (requested_amount BETWEEN 1 AND 99999),
    CONSTRAINT ash_result_code_known CHECK (result_code BETWEEN 0 AND 2),
    CONSTRAINT ash_result_rejection_kind CHECK (
        result_code = 0
        OR (result_code = 1 AND mutation_kind = 1)
        OR (result_code = 2 AND mutation_kind = 0)
    ),
    CONSTRAINT ash_result_balances_bounded CHECK (
        before_balance BETWEEN 0 AND 99999
        AND after_balance BETWEEN 0 AND 99999
    ),
    CONSTRAINT ash_result_versions_shape CHECK (
        pre_wallet_version > 0
        AND (
            (result_code = 0 AND post_wallet_version = pre_wallet_version + 1)
            OR (result_code <> 0 AND post_wallet_version = pre_wallet_version)
        )
    ),
    CONSTRAINT ash_result_arithmetic CHECK (
        (result_code = 0 AND mutation_kind = 0
            AND after_balance = before_balance + requested_amount)
        OR (result_code = 0 AND mutation_kind = 1
            AND after_balance = before_balance - requested_amount)
        OR (result_code <> 0 AND after_balance = before_balance)
    )
);

CREATE TABLE currency_ledger_events (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    event_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    currency_id TEXT NOT NULL,
    reason_code TEXT NOT NULL,
    source_id TEXT NOT NULL,
    content_version TEXT NOT NULL,
    before_balance INTEGER NOT NULL,
    delta INTEGER NOT NULL,
    after_balance INTEGER NOT NULL,
    wallet_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, event_id),
    UNIQUE (namespace_id, account_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id, mutation_id)
        REFERENCES ash_mutation_results(namespace_id, account_id, mutation_id) ON DELETE CASCADE,
    CONSTRAINT currency_ledger_account_id_exact CHECK (octet_length(account_id) = 16),
    CONSTRAINT currency_ledger_event_id_exact CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT currency_ledger_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT currency_ledger_currency_exact CHECK (currency_id = 'currency.ash_shards'),
    CONSTRAINT currency_ledger_reason_bounded CHECK (char_length(reason_code) BETWEEN 1 AND 64),
    CONSTRAINT currency_ledger_source_bounded CHECK (char_length(source_id) BETWEEN 1 AND 128),
    CONSTRAINT currency_ledger_content_bounded CHECK (char_length(content_version) BETWEEN 1 AND 128),
    CONSTRAINT currency_ledger_balances_bounded CHECK (
        before_balance BETWEEN 0 AND 99999
        AND after_balance BETWEEN 0 AND 99999
    ),
    CONSTRAINT currency_ledger_delta_nonzero CHECK (delta BETWEEN -99999 AND 99999 AND delta <> 0),
    CONSTRAINT currency_ledger_arithmetic CHECK (after_balance = before_balance + delta),
    CONSTRAINT currency_ledger_version_positive CHECK (wallet_version > 1)
);

CREATE INDEX currency_ledger_account_history
    ON currency_ledger_events (namespace_id, account_id, wallet_version, event_id);
