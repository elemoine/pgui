# pgui

A minimal terminal UI for PostgreSQL. Browse tables, inspect columns, and run SQL — all from the keyboard.

![pgui screenshot](./screenshot.png)

## Features

- Browse and filter the tables in your database
- Inspect a table's columns (name, type, nullability)
- Write and run SQL queries, with scrollable results

## Usage

Set the `DATABASE_URL` environment variable to your PostgreSQL connection string, then run:

```sh
DATABASE_URL=postgres://user:password@localhost:5432/dbname cargo run
```

## License

Licensed under the MIT license ([LICENSE](./LICENSE)).

Copyright (c) Éric Lemoine <eric.lemoine@gmail.com>
