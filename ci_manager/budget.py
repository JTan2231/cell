"""Derive repair charges from the frozen policy and retained attempts."""


def repair_budget(job: dict) -> dict:
    policy = job["policy"]
    attempts = job.get("attempts", [])
    refundable = policy.get("refund_accepted_patches") is True
    refunded = sum(bool(attempt.get("candidate")) for attempt in attempts) if refundable else 0
    total = policy["luna_attempts"] + policy["terra_attempts"]
    used = len(attempts) - refunded
    return {"mode": "refund_accepted" if refundable else "invocations",
            "total": total, "used": used, "remaining": max(0, total - used),
            "refunded": refunded, "invocations": len(attempts)}
