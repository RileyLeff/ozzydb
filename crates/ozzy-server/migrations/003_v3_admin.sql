-- v3: Admin flag for users
ALTER TABLE users ADD COLUMN is_admin BOOLEAN NOT NULL DEFAULT false;
