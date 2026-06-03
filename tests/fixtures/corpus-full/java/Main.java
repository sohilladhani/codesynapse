package com.example.app;

public class Main {
    public static void main(String[] args) {
        Database db = new Database();
        db.connect();

        UserService userService = new UserService(db);
        User user = userService.register("alice", "alice@example.com");
        System.out.println("Registered: " + user.displayName());

        db.disconnect();
    }
}
