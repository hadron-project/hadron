use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;

/*
TODO:
- need to update the way the system treats input id & source.
- finish publishing the beta of the Rust hadron client library.

Time to implement. Our microservice will have a live subscription to our Stream `events`,
and when an event is received, the algorithm of the event handler will be roughly as follows:

- Open a new transaction with our database.
- Attempt to write a new row to the in-table using the event's `id` and `source`
  fields as values for their respective columns.
    - If an error is returned indicating a primary key violation, then we know
      that we've already processed this event. Close the database transaction. Return a
      success response from the event handler. Done.
    - Else, if no error, then continue.
- Now time for business logic. Do whatever it is your microservice needs to do.
  Manipulate some rows in the database using the open transaction. Whatever.
- If your business logic needs to produce a new event as part of its business
  logic, then craft the new event, and write it to the out-table.
- Commit the database transaction. If errors take place, no worries. Just return the error from the event handler and a retry will take place.
- Finally, return a success from the event handler. Done!

*/

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect("postgres://postgres:password@localhost/test")
        .await?;

    // Make a simple query to return the given parameter (use a question mark `?` instead of `$1` for MySQL)
    let row: (i64,) = sqlx::query_as("SELECT $1").bind(150_i64).fetch_one(&pool).await?;

    assert_eq!(row.0, 150);

    Ok(())
}
