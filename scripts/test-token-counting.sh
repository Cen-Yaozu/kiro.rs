#!/bin/bash

# Token Counting API Test Script
# This script tests the count_tokens API configuration

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
KIRO_HOST="${KIRO_HOST:-127.0.0.1}"
KIRO_PORT="${KIRO_PORT:-8990}"
KIRO_API_KEY="${KIRO_API_KEY:-sk-kiro-rs-qazWSXedcRFV123456}"

echo "========================================="
echo "Token Counting API Test"
echo "========================================="
echo ""

# Test 1: Check if kiro.rs is running
echo -e "${YELLOW}Test 1: Checking if kiro.rs is running...${NC}"
if curl -s -f "http://${KIRO_HOST}:${KIRO_PORT}/v1/models" \
  -H "x-api-key: ${KIRO_API_KEY}" > /dev/null 2>&1; then
  echo -e "${GREEN}✓ kiro.rs is running${NC}"
else
  echo -e "${RED}✗ kiro.rs is not running or not accessible${NC}"
  echo "Please start kiro.rs first: ./target/release/kiro-rs"
  exit 1
fi
echo ""

# Test 2: Test count_tokens endpoint
echo -e "${YELLOW}Test 2: Testing count_tokens endpoint...${NC}"
RESPONSE=$(curl -s "http://${KIRO_HOST}:${KIRO_PORT}/v1/messages/count_tokens" \
  -H "Content-Type: application/json" \
  -H "x-api-key: ${KIRO_API_KEY}" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "messages": [
      {"role": "user", "content": "Hello, Claude! How are you today?"}
    ]
  }')

if echo "$RESPONSE" | jq -e '.input_tokens' > /dev/null 2>&1; then
  TOKEN_COUNT=$(echo "$RESPONSE" | jq '.input_tokens')
  echo -e "${GREEN}✓ count_tokens endpoint works${NC}"
  echo "  Token count: $TOKEN_COUNT"
else
  echo -e "${RED}✗ count_tokens endpoint failed${NC}"
  echo "  Response: $RESPONSE"
  exit 1
fi
echo ""

# Test 3: Check logs for remote API usage
echo -e "${YELLOW}Test 3: Checking if remote API is being used...${NC}"
echo "Please check your kiro.rs logs for one of these messages:"
echo "  - ${GREEN}DEBUG kiro_rs::token: 远程 count_tokens API 返回: <number>${NC}"
echo "    (This means remote API is working)"
echo "  - ${YELLOW}WARN kiro_rs::token: 远程 count_tokens API 调用失败，回退到本地计算${NC}"
echo "    (This means falling back to local estimation)"
echo ""
echo "If you don't see either message, remote API is not configured."
echo ""

# Test 4: Test with longer message
echo -e "${YELLOW}Test 4: Testing with longer message...${NC}"
LONG_MESSAGE="This is a longer test message to verify token counting accuracy. "
LONG_MESSAGE="${LONG_MESSAGE}${LONG_MESSAGE}${LONG_MESSAGE}${LONG_MESSAGE}"

RESPONSE=$(curl -s "http://${KIRO_HOST}:${KIRO_PORT}/v1/messages/count_tokens" \
  -H "Content-Type: application/json" \
  -H "x-api-key: ${KIRO_API_KEY}" \
  -d "{
    \"model\": \"claude-sonnet-4-20250514\",
    \"messages\": [
      {\"role\": \"user\", \"content\": \"${LONG_MESSAGE}\"}
    ]
  }")

if echo "$RESPONSE" | jq -e '.input_tokens' > /dev/null 2>&1; then
  TOKEN_COUNT=$(echo "$RESPONSE" | jq '.input_tokens')
  echo -e "${GREEN}✓ Longer message test passed${NC}"
  echo "  Token count: $TOKEN_COUNT"
else
  echo -e "${RED}✗ Longer message test failed${NC}"
  echo "  Response: $RESPONSE"
  exit 1
fi
echo ""

# Summary
echo "========================================="
echo -e "${GREEN}All tests passed!${NC}"
echo "========================================="
echo ""
echo "Next steps:"
echo "1. Check kiro.rs logs to verify if remote API is being used"
echo "2. If using remote API, you should see: 'DEBUG kiro_rs::token: 远程 count_tokens API 返回: <number>'"
echo "3. If not configured, consider adding count_tokens configuration to config.json"
echo ""
echo "For more information, see: docs/token-counting-testing.md"
