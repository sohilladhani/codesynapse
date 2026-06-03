package com.example.app;

import java.util.Optional;

public class UserService {
    private final Database db;

    public UserService(Database db) {
        this.db = db;
    }

    public User register(String username, String email) {
        User user = new User((int) System.currentTimeMillis(), username, email);
        db.saveUser(user);
        return user;
    }

    public Optional<User> findById(int id) {
        return db.findUser(id);
    }

    public boolean deactivate(int id) {
        Optional<User> user = db.findUser(id);
        if (user.isEmpty()) return false;
        user.get().deactivate();
        db.saveUser(user.get());
        return true;
    }
}
