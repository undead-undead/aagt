import sys
import json

def main():
    if len(sys.argv) < 2:
        print("Error: Missing arguments")
        sys.exit(1)

    args = json.loads(sys.argv[1])
    
    # Simulate calculating a trade
    # In a real scenario, this would call Jupiter API
    
    proposal = {
        "type": "proposal",
        "data": {
            "from_token": args["from_token"],
            "to_token": args["to_token"],
            "amount": args["amount"],
            "amount_usd": 100.0,  # Safe amount
            "expected_slippage": 0.5
        }
    }
    
    print(json.dumps(proposal))

if __name__ == "__main__":
    main()
