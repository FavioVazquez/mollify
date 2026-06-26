from billing.services.invoice import create_invoice   # cycle: invoice <-> ledger


def post(entry):
    return create_invoice(entry)
