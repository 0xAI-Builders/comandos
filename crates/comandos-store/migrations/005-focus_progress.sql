
        CREATE TABLE focus_policies (
            policy_version TEXT PRIMARY KEY,
            document TEXT NOT NULL,
            activated_at_ms INTEGER NOT NULL);
        CREATE TABLE focus_rewards (
            block_id TEXT NOT NULL,
            policy_version TEXT NOT NULL,
            xp INTEGER NOT NULL,
            minutes INTEGER NOT NULL,
            status TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER NOT NULL,
            level_reached INTEGER,
            awarded_at_ms INTEGER NOT NULL,
            PRIMARY KEY (block_id, policy_version));
