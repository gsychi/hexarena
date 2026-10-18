# ADR 0003: Placement-based scoring

Status: accepted

Matches award `N - place + 1` points based on elimination order, with ties sharing average places. Ratings must use full finishing ranks (e.g. a Plackett-Luce style model), never win/loss alone. Trainers center or normalize terminal rewards internally; raw 1..N points are for display.
