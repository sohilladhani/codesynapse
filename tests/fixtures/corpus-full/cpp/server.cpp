#include <iostream>
#include <string>
#include <vector>
#include <unordered_map>
#include <functional>

class Request {
public:
    std::string method;
    std::string path;
    std::string body;

    Request(std::string method, std::string path, std::string body = "")
        : method(std::move(method)), path(std::move(path)), body(std::move(body)) {}
};

class Response {
public:
    int statusCode;
    std::string body;

    Response(int status, std::string body)
        : statusCode(status), body(std::move(body)) {}

    static Response ok(const std::string& body) { return {200, body}; }
    static Response notFound() { return {404, "{\"error\":\"not found\"}"}; }
    static Response badRequest() { return {400, "{\"error\":\"bad request\"}"}; }
};

using Handler = std::function<Response(const Request&)>;

class Router {
    std::unordered_map<std::string, Handler> routes;

public:
    void get(const std::string& path, Handler handler) {
        routes["GET:" + path] = std::move(handler);
    }

    void post(const std::string& path, Handler handler) {
        routes["POST:" + path] = std::move(handler);
    }

    Response handle(const Request& req) const {
        auto it = routes.find(req.method + ":" + req.path);
        if (it == routes.end()) return Response::notFound();
        return it->second(req);
    }
};

class Server {
    Router router;
    int port;

public:
    explicit Server(int port) : port(port) {}

    void configure(Router r) { router = std::move(r); }

    void start() {
        std::cout << "Server listening on port " << port << std::endl;
    }
};
