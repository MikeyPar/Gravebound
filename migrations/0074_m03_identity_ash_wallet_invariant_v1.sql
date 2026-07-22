-- GB-M03 production-route aggregate repair.
--
-- The private-life bootstrap requires every account to own one Ash wallet. Identity transactions
-- now establish that component under the already-locked account row; this additive migration
-- repairs wipeable accounts created before that invariant without changing an existing wallet.

INSERT INTO ash_wallets (namespace_id, account_id, balance, wallet_version)
SELECT account.namespace_id, account.account_id, 0, 1
FROM accounts AS account
WHERE account.namespace_id = 'core-dev'
  AND NOT EXISTS (
      SELECT 1
      FROM ash_wallets AS wallet
      WHERE wallet.namespace_id = account.namespace_id
        AND wallet.account_id = account.account_id
  )
ON CONFLICT (namespace_id, account_id) DO NOTHING;

COMMENT ON TABLE ash_wallets IS
    'Account-owned Ash wallet; schema 74 repairs the required private-life component for pre-74 wipeable identity aggregates.';
