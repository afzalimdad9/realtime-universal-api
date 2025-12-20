import http from "k6/http";
import { check, sleep } from "k6";

export let options = {
  stages: [
    { duration: "30s", target: 20 },
    { duration: "1m", target: 50 },
    { duration: "30s", target: 0 },
  ],
};

const BASE_URL = "http://localhost:3000";

export default function () {
  // Health check
  let healthRes = http.get(`${BASE_URL}/health`);
  check(healthRes, {
    "health check status is 200": (r) => r.status === 200,
  });

  // Test API endpoints
  let apiRes = http.get(`${BASE_URL}/api/v1/tenants`);
  check(apiRes, {
    "tenants endpoint responds": (r) => r.status === 200 || r.status === 401,
  });

  // Test GraphQL endpoint
  let graphqlPayload = JSON.stringify({
    query: "{ __schema { types { name } } }",
  });

  let graphqlRes = http.post(`${BASE_URL}/graphql`, graphqlPayload, {
    headers: { "Content-Type": "application/json" },
  });

  check(graphqlRes, {
    "graphql endpoint responds": (r) => r.status === 200 || r.status === 401,
  });

  sleep(1);
}
