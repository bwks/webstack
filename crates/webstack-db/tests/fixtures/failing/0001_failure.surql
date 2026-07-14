CREATE rollback_event SET created = true;
THROW "migration failed";
