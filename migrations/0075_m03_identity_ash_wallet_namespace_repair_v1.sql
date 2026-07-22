-- GB-M03 production-route aggregate repair, authoritative namespace correction.
--
-- Published schema 74 used the content bundle label instead of the durable wipeable namespace.
-- Keep that migration immutable and additively establish the missing account-owned component here.

INSERT INTO ash_wallets (namespace_id, account_id, balance, wallet_version)
SELECT account.namespace_id, account.account_id, 0, 1
FROM accounts AS account
WHERE account.namespace_id = 'test.core'
  AND NOT EXISTS (
      SELECT 1
      FROM ash_wallets AS wallet
      WHERE wallet.namespace_id = account.namespace_id
        AND wallet.account_id = account.account_id
  )
ON CONFLICT (namespace_id, account_id) DO NOTHING;

COMMENT ON TABLE ash_wallets IS
    'Account-owned Ash wallet; schema 75 additively repairs missing wallets in the authoritative test.core namespace.';
