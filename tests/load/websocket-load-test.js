import ws from "k6/ws";
import { check } from "k6";

export let options = {
  stages: [
    { duration: "30s", target: 20 },
    { duration: "1m", target: 50 },
    { duration: "30s", target: 0 },
  ],
};

export default function () {
  const url = "ws://localhost:3000/ws";
  const params = { tags: { my_tag: "websocket" } };

  const res = ws.connect(url, params, function (socket) {
    socket.on("open", () => {
      console.log("Connected");

      // Send a test message
      socket.send(
        JSON.stringify({
          type: "subscribe",
          topic: "test-topic",
          tenant_id: "test-tenant",
        })
      );
    });

    socket.on("message", (data) => {
      console.log("Message received: ", data);
    });

    socket.on("close", () => {
      console.log("Disconnected");
    });

    socket.setTimeout(() => {
      socket.close();
    }, 10000);
  });

  check(res, { "status is 101": (r) => r && r.status === 101 });
}
