-- A study's pull that has not ended whole, by the study's id (placement's Definition::id): when the
-- first one since its last whole pull began. An answer fetched before it is bought again; one since is
-- what a stopped pull already paid for.
CREATE TABLE placement_pulls (
	study TEXT PRIMARY KEY CHECK (length(study) = 64),
	since INTEGER NOT NULL
) STRICT;
