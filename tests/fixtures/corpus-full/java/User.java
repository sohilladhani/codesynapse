package com.example.app;

import java.time.Instant;

public class User {
    private final int id;
    private final String username;
    private final String email;
    private final Instant createdAt;
    private boolean isActive;

    public User(int id, String username, String email) {
        this.id = id;
        this.username = username;
        this.email = email;
        this.createdAt = Instant.now();
        this.isActive = true;
    }

    public int getId() { return id; }
    public String getUsername() { return username; }
    public String getEmail() { return email; }
    public boolean isActive() { return isActive; }

    public String displayName() {
        return username;
    }

    public void deactivate() {
        this.isActive = false;
    }
}
