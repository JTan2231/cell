# Mantic

Mantic forecasts the money remaining after expected expenses. One private
SQLite database holds named configs and their expense items. Starting amounts,
forecast periods, and results belong to one command and are not retained.

```sh
mantic init
mantic config create household
mantic item add household rent 1500 --first 2026-10-01 --every month
mantic forecast household 5000 --from 2026-10-02 --until 2026-11-02
```

- [Product overview and feature inventory](chancery/overview.md)
- [Manage configs and expense items](chancery/manuals/config-manage.md)
- [Calculate a forecast](chancery/manuals/forecast-calculate.md)
- [Installation and private state](chancery/manuals/installation.md)
- [Install and recover Mantic](chancery/manuals/install-operate.md)

Read `chancery product mantic` for the installed publication. Use
`chancery show ID` for one contract and `chancery resolve ID` for its required
contracts and compatibility gaps.
