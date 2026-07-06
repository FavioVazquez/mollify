import os
import hashlib
from billing.services.invoice import create_invoice
from billing.domain.money import Money


def main(amount: int) -> None:
    inv = create_invoice(Money(amount))
    print(inv)


def _legacy_helper(x):          # never referenced anywhere
    tmp = x + 1
    return x


def password_hash(p):           # weak hash (security)
    return hashlib.md5(p.encode()).hexdigest()



if __name__ == "__main__":
    main(100)
