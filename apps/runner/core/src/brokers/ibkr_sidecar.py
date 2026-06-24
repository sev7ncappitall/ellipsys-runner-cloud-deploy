from __future__ import annotations

import asyncio
import json
import sys
from typing import Any

try:
    from ib_insync import Forex, IB, LimitOrder, MarketOrder, Stock, StopOrder
except Exception as exc:  # pragma: no cover - depends on the local Python runtime
    IB = None
    Forex = None
    Stock = None
    MarketOrder = None
    LimitOrder = None
    StopOrder = None
    IMPORT_ERROR = str(exc)
else:
    IMPORT_ERROR = None

STATUS_MAP = {
    "filled": "FILLED",
    "partiallyfilled": "PARTIALLY_FILLED",
    "submitted": "OPEN",
    "cancelled": "CANCELLED",
    "inactive": "REJECTED",
    "pendingcancel": "PENDING_CANCEL",
    "presubmitted": "PENDING_NEW",
}


def emit(payload: dict[str, Any], exit_code: int = 0) -> None:
    sys.stdout.write(json.dumps(payload))
    raise SystemExit(exit_code)


def contract_for(symbol: str):
    normalized = symbol.upper()
    if len(normalized) == 6 and normalized.isalpha():
        return Forex(normalized)
    return Stock(normalized, "SMART", "USD")


def order_for(order: dict[str, Any]):
    side = str(order.get("side", "buy")).upper()
    quantity = float(order.get("quantity", 0) or 0)
    order_type = str(order.get("order_type") or order.get("orderType") or "market").lower()
    limit_price = order.get("limit_price") or order.get("limitPrice")
    stop_price = order.get("stop_price") or order.get("stopPrice") or order.get("stop_loss") or order.get("stopLoss")

    if order.get("stop_loss") is not None or order.get("take_profit") is not None or order.get("stopLoss") is not None or order.get("takeProfit") is not None:
        emit(
            {
                "ok": False,
                "status": "rejected",
                "error": "IBKR sidecar does not attach stop-loss/take-profit legs yet. Submit a simple market, limit, or stop order.",
            },
            exit_code=1,
        )

    if order_type == "limit":
        if limit_price is None:
            emit({"ok": False, "status": "rejected", "error": "IBKR limit orders require limit_price"}, exit_code=1)
        ib_order = LimitOrder(side, quantity, float(limit_price))
    elif order_type == "stop":
        if stop_price is None:
            emit({"ok": False, "status": "rejected", "error": "IBKR stop orders require stop_loss/stop_price"}, exit_code=1)
        ib_order = StopOrder(side, quantity, float(stop_price))
    else:
        ib_order = MarketOrder(side, quantity)

    client_order_id = order.get("client_order_id") or order.get("clientOrderId")
    if client_order_id:
        ib_order.orderRef = str(client_order_id)[:32]
    return ib_order


async def load_account_snapshot(ib: Any, account_id: str | None) -> dict[str, Any]:
    summary = await ib.accountSummaryAsync()
    values = {row.tag: row.value for row in summary}
    return {
        "ok": True,
        "account_id": account_id,
        "balance": float(values.get("NetLiquidation", 0.0) or 0.0),
        "buying_power": float(values.get("BuyingPower", 0.0) or 0.0),
    }


async def main() -> None:
    if IB is None:
        emit(
            {
                "ok": False,
                "status": "error",
                "error": f"ib_insync is not available in this Python runtime: {IMPORT_ERROR}",
            },
            exit_code=1,
        )

    request = json.loads(sys.stdin.read() or "{}")
    credentials = request.get("credentials") or {}
    is_paper = bool(request.get("is_paper", True))
    host = str(credentials.get("host") or "127.0.0.1")
    port = int(credentials.get("port") or (7497 if is_paper else 7496))
    client_id = int(credentials.get("client_id") or 10)
    action = str(request.get("action") or "")

    ib = IB()
    try:
        await ib.connectAsync(host, port, clientId=client_id, timeout=10)
        accounts = ib.managedAccounts()
        account_id = accounts[0] if accounts else None

        if action == "authenticate":
            emit(await load_account_snapshot(ib, account_id))

        if action == "get_account":
            emit(await load_account_snapshot(ib, account_id))

        if action == "submit_order":
            order = request.get("order") or {}
            contract = contract_for(str(order.get("symbol") or ""))
            await ib.qualifyContractsAsync(contract)
            ib_order = order_for(order)
            trade = ib.placeOrder(contract, ib_order)
            await asyncio.sleep(0.5)
            status = STATUS_MAP.get(str(trade.orderStatus.status).lower(), "PENDING_NEW")
            emit(
                {
                    "ok": status != "REJECTED",
                    "broker_order_id": str(trade.order.orderId),
                    "status": status,
                    "error": None if status != "REJECTED" else str(trade.orderStatus.status),
                },
                exit_code=0 if status != "REJECTED" else 1,
            )

        emit({"ok": False, "status": "error", "error": f"Unknown IBKR action: {action}"}, exit_code=1)
    except Exception as exc:
        emit(
            {
                "ok": False,
                "status": "error",
                "error": f"Unable to connect to IBKR TWS/IB Gateway on {host}:{port}: {exc}",
            },
            exit_code=1,
        )
    finally:
        if ib.isConnected():
            ib.disconnect()


if __name__ == "__main__":
    asyncio.run(main())
