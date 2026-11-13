#!/usr/bin/env python3
"""Cost guard for the production acceptance. Refuses when the projected cost passes the cap. Starts no resource."""
import argparse, math, sys

def require(condition, message):
    if not condition: raise ValueError(message)

def number(value, name, positive=False):
    require(type(value) in (int, float) and math.isfinite(value), name + ' must be a finite non-negative number')
    require(value >= 0, name + ' must be a finite non-negative number')
    if positive:
        require(value > 0, name + ' must be a finite non-negative number')
    return value

def projected_cost(spent_usd: float, hourly_usd: float, hours_left: float) -> float:
    return spent_usd + hourly_usd * hours_left

def check(cap_usd: float, spent_usd: float, hourly_usd: float, hours_left: float) -> None:
    number(cap_usd, 'cap_usd', positive=True)
    number(spent_usd, 'spent_usd')
    number(hourly_usd, 'hourly_usd')
    number(hours_left, 'hours_left')
    p = projected_cost(spent_usd, hourly_usd, hours_left)
    if p > cap_usd:
        raise ValueError(f'cost cap exceeded: projected {p:.2f} USD > cap {cap_usd:.2f} USD')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cap-usd', type=float, required=True)
    parser.add_argument('--spent-usd', type=float, required=True)
    parser.add_argument('--hourly-usd', type=float, required=True)
    parser.add_argument('--hours-left', type=float, required=True)
    args = parser.parse_args()
    try:
        check(args.cap_usd, args.spent_usd, args.hourly_usd, args.hours_left)
    except ValueError as error:
        print(error, file=sys.stderr)
        raise SystemExit(1)
    p = projected_cost(args.spent_usd, args.hourly_usd, args.hours_left)
    print(f'cost guard ok: projected {p:.2f} USD <= cap {args.cap_usd:.2f} USD')

if __name__ == '__main__':
    main()
