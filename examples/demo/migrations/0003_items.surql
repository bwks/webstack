-- Add the shared reference Items feature.
DEFINE TABLE item SCHEMAFULL;
DEFINE FIELD name ON item TYPE string ASSERT string::len($value) >= 1 AND string::len($value) <= 100;
DEFINE FIELD created_at ON item TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON item TYPE datetime DEFAULT time::now() VALUE time::now();
