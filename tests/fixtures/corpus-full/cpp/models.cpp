#include <string>
#include <vector>
#include <optional>
#include <chrono>

struct User {
    int id;
    std::string username;
    std::string email;
    bool isActive;

    User(int id, std::string username, std::string email)
        : id(id), username(std::move(username)), email(std::move(email)), isActive(true) {}

    std::string displayName() const { return username; }
    void deactivate() { isActive = false; }
};

struct Product {
    int id;
    std::string name;
    double price;
    int stock;
    std::optional<std::string> category;

    Product(int id, std::string name, double price)
        : id(id), name(std::move(name)), price(price), stock(0) {}

    bool isAvailable() const { return stock > 0; }
    double applyDiscount(double pct) const { return price * (1.0 - pct / 100.0); }
};

struct OrderItem {
    int productId;
    int quantity;
    double price;
};

struct Order {
    int id;
    int userId;
    std::vector<OrderItem> items;
    double total;
    std::string status;

    Order(int id, int userId)
        : id(id), userId(userId), total(0.0), status("pending") {}

    void addItem(const OrderItem& item) {
        items.push_back(item);
        total += item.price * item.quantity;
    }
};
