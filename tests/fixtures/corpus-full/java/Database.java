package com.example.app;

import java.util.HashMap;
import java.util.Map;
import java.util.Optional;

public class Database {
    private final Map<String, Object> store = new HashMap<>();

    public void connect() {}
    public void disconnect() {}

    public void saveUser(User user) {
        store.put("user:" + user.getId(), user);
    }

    public Optional<User> findUser(int id) {
        return Optional.ofNullable((User) store.get("user:" + id));
    }

    public void saveProduct(Product product) {
        store.put("product:" + product.getId(), product);
    }

    public Optional<Product> findProduct(int id) {
        return Optional.ofNullable((Product) store.get("product:" + id));
    }
}
