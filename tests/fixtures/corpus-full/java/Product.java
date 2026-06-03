package com.example.app;

import java.util.Optional;

public class Product {
    private final int id;
    private final String name;
    private double price;
    private int stock;
    private String category;

    public Product(int id, String name, double price) {
        this.id = id;
        this.name = name;
        this.price = price;
        this.stock = 0;
    }

    public int getId() { return id; }
    public String getName() { return name; }
    public double getPrice() { return price; }
    public int getStock() { return stock; }
    public Optional<String> getCategory() { return Optional.ofNullable(category); }

    public boolean isAvailable() {
        return stock > 0;
    }

    public double applyDiscount(double pct) {
        return price * (1 - pct / 100);
    }

    public void restock(int qty) {
        this.stock += qty;
    }
}
