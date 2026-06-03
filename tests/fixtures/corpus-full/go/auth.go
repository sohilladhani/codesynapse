package main

import (
	"crypto/rand"
	"encoding/hex"
	"sync"
)

type AuthManager struct {
	db        *Database
	secretKey string
	mu        sync.RWMutex
	sessions  map[string]int
}

func NewAuthManager(db *Database, secretKey string) *AuthManager {
	return &AuthManager{db: db, secretKey: secretKey, sessions: make(map[string]int)}
}

func (a *AuthManager) Login(username, password string) (string, bool) {
	token := generateToken()
	a.mu.Lock()
	defer a.mu.Unlock()
	a.sessions[token] = 0
	return token, true
}

func (a *AuthManager) Logout(token string) {
	a.mu.Lock()
	defer a.mu.Unlock()
	delete(a.sessions, token)
}

func (a *AuthManager) GetUser(token string) (*User, bool) {
	a.mu.RLock()
	userID, ok := a.sessions[token]
	a.mu.RUnlock()
	if !ok {
		return nil, false
	}
	return a.db.FindUser(userID)
}

func generateToken() string {
	b := make([]byte, 32)
	rand.Read(b)
	return hex.EncodeToString(b)
}
