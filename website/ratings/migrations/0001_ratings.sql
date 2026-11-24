-- Mister Download Manager's star ratings (src/ratings.js).

-- One row per visitor: `voter` is HMAC-SHA256(IP_SALT, the visitor's IP address), never the
-- address; `stars` is 1 to 5. Nothing else is kept about a rating.
CREATE TABLE ratings (
  voter TEXT PRIMARY KEY NOT NULL,
  stars INTEGER NOT NULL CHECK (stars BETWEEN 1 AND 5)
) WITHOUT ROWID;

-- How many ratings give each number of stars, kept by the triggers below in the same statement
-- as the rating, so reading the numbers reads five rows, never every rating.
CREATE TABLE totals (
  stars INTEGER PRIMARY KEY CHECK (stars BETWEEN 1 AND 5),
  n INTEGER NOT NULL DEFAULT 0 CHECK (n >= 0)
);
INSERT INTO totals (stars, n) VALUES (1, 0), (2, 0), (3, 0), (4, 0), (5, 0);

CREATE TRIGGER ratings_added AFTER INSERT ON ratings
BEGIN
  UPDATE totals SET n = n + 1 WHERE stars = NEW.stars;
END;

CREATE TRIGGER ratings_changed AFTER UPDATE OF stars ON ratings
WHEN OLD.stars <> NEW.stars
BEGIN
  UPDATE totals SET n = n - 1 WHERE stars = OLD.stars;
  UPDATE totals SET n = n + 1 WHERE stars = NEW.stars;
END;

CREATE TRIGGER ratings_removed AFTER DELETE ON ratings
BEGIN
  UPDATE totals SET n = n - 1 WHERE stars = OLD.stars;
END;

-- Rating attempts, for the limit per visitor (the same hash, and when); deleted after an hour.
CREATE TABLE attempts (
  voter TEXT NOT NULL,
  at INTEGER NOT NULL
);
CREATE INDEX attempts_by_voter ON attempts (voter, at);
CREATE INDEX attempts_by_time ON attempts (at);
