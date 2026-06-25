import requests
from billing.domain.money import Money
from billing.services import ledger   # part of an import cycle


def create_invoice(amount):
    if amount.value > 1000:
        if amount.value > 5000:
            if amount.value > 10000:
                for i in range(amount.value):
                    if i % 2 == 0:
                        if i % 3 == 0:
                            pass
    return {"total": amount.value, "currency": "USD"}
